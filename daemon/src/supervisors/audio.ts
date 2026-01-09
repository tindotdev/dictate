import { EventEmitter } from "node:events";
import type { Subprocess } from "bun";
import type { AudioBackend } from "../audio/index.js";
import { createAudioBackend } from "../audio/index.js";
import {
	type BackoffConfig,
	type BackoffState,
	createBackoffState,
	nextBackoff,
	resetBackoff,
} from "../backoff.js";

// ============================================================================
// Audio Constants (OpenAI Realtime API requirements)
// ============================================================================

const SAMPLE_RATE = 24000;
const CHANNELS = 1;
const BYTES_PER_SAMPLE = 2; // s16 = 16-bit = 2 bytes
const FRAME_MS = 20; // 20ms frames
const FRAME_BYTES =
	(SAMPLE_RATE * BYTES_PER_SAMPLE * CHANNELS * FRAME_MS) / 1000; // 960 bytes

export const AUDIO_CONSTANTS = {
	SAMPLE_RATE,
	CHANNELS,
	BYTES_PER_SAMPLE,
	FRAME_MS,
	FRAME_BYTES,
} as const;

// ============================================================================
// Supervisor Types
// ============================================================================

export type AudioSupervisorState =
	| "stopped"
	| "starting"
	| "running"
	| "restarting"
	| "failed";

export interface AudioSupervisorEvents {
	chunk: (chunk: Buffer) => void;
	started: () => void;
	stopped: () => void;
	error: (error: Error) => void;
	restarting: (attempt: number, delayMs: number) => void;
	failed: (error: Error) => void;
	state_change: (from: AudioSupervisorState, to: AudioSupervisorState) => void;
}

export declare interface AudioSupervisor {
	on<K extends keyof AudioSupervisorEvents>(
		event: K,
		listener: AudioSupervisorEvents[K],
	): this;
	emit<K extends keyof AudioSupervisorEvents>(
		event: K,
		...args: Parameters<AudioSupervisorEvents[K]>
	): boolean;
}

export interface AudioSupervisorOptions {
	backoff?: Partial<BackoffConfig>;
	/** Audio backend to use (default: auto-detect based on platform) */
	backend?: AudioBackend;
}

// ============================================================================
// Audio Supervisor
// ============================================================================

export class AudioSupervisor extends EventEmitter {
	private process: Subprocess | null = null;
	private buffer: Buffer = Buffer.alloc(0);
	private stdoutReader: ReadableStreamDefaultReader<Uint8Array> | null = null;
	private state: AudioSupervisorState = "stopped";
	private intentionalStop = false;
	private backoffState: BackoffState;
	private restartTimer: ReturnType<typeof setTimeout> | null = null;
	private backend: AudioBackend;

	constructor(options: AudioSupervisorOptions = {}) {
		super();
		this.backoffState = createBackoffState(options.backoff);
		this.backend = options.backend ?? createAudioBackend();
	}

	private setState(newState: AudioSupervisorState): void {
		if (this.state !== newState) {
			const from = this.state;
			this.state = newState;
			this.emit("state_change", from, newState);
		}
	}

	getState(): AudioSupervisorState {
		return this.state;
	}

	isRunning(): boolean {
		return this.state === "running";
	}

	getBackendName(): string {
		return this.backend.name;
	}

	/**
	 * Validate that the audio backend is available.
	 * Returns null if valid, or an error message if not.
	 */
	async validate(): Promise<string | null> {
		return this.backend.validate();
	}

	/**
	 * Start audio capture. If already running, this is a no-op.
	 */
	start(): void {
		if (this.state === "running" || this.state === "starting") {
			return;
		}

		this.intentionalStop = false;
		this.cancelRestart();
		this.spawnProcess();
	}

	/**
	 * Stop audio capture. Prevents automatic restart.
	 */
	stop(): void {
		// Set intentionalStop FIRST before any other operations
		// to prevent race conditions with process exit handlers
		this.intentionalStop = true;
		this.cancelRestart();
		this.killProcess();
		resetBackoff(this.backoffState);

		// Only transition to stopped if not already stopped
		if (this.state !== "stopped") {
			this.setState("stopped");
			this.emit("stopped");
		}
	}

	private spawnProcess(): void {
		this.setState("starting");

		const { command, args, env } = this.backend.getCommand();

		try {
			this.process = Bun.spawn([command, ...args], {
				stdout: "pipe",
				stderr: "pipe",
				env: env ? { ...process.env, ...env } : undefined,
				onExit: (_proc, exitCode, _signalCode, error) => {
					// Clean up reader reference
					this.stdoutReader = null;

					if (error) {
						this.emit("error", error);
						this.handleExit(-1, error);
					} else {
						this.handleExit(exitCode ?? 0);
					}
				},
			});
		} catch (err) {
			// Spawn failed synchronously (e.g., ENOENT)
			this.emit("error", err as Error);
			this.handleExit(-1, err as Error);
			return;
		}

		// Check if process has a valid pid (spawned successfully)
		if (this.process.pid !== undefined) {
			this.setState("running");
			this.emit("started");
		}

		// Start reading stdout asynchronously
		this.readStdout();
	}

	private async readStdout(): Promise<void> {
		const stdout = this.process?.stdout;
		if (!stdout || typeof stdout === "number") return;

		try {
			this.stdoutReader = stdout.getReader();

			while (true) {
				const { done, value } = await this.stdoutReader.read();
				if (done) break;

				// Handle the chunk
				this.buffer = Buffer.concat([this.buffer, Buffer.from(value)]);

				// Emit complete frames
				while (this.buffer.length >= FRAME_BYTES) {
					const frame = this.buffer.subarray(0, FRAME_BYTES);
					this.buffer = this.buffer.subarray(FRAME_BYTES);
					this.emit("chunk", frame);
				}
			}
		} catch (err) {
			// Reader cancelled or error - ignore if intentional stop
			if (!this.intentionalStop && err instanceof Error) {
				this.emit("error", err);
			}
		}
	}

	private handleExit(_code: number, error?: Error): void {
		this.process = null;
		this.buffer = Buffer.alloc(0);

		// If intentionally stopped, don't restart
		if (this.intentionalStop) {
			return;
		}

		// Unexpected exit - attempt restart with backoff
		const delayMs = nextBackoff(this.backoffState);

		if (delayMs === null) {
			// Max retries exceeded
			this.setState("failed");
			this.emit(
				"failed",
				error ??
					new Error(
						`${this.backend.name} failed after ${this.backoffState.config.maxRetries} retries`,
					),
			);
			return;
		}

		this.setState("restarting");
		this.emit("restarting", this.backoffState.attempt, delayMs);

		this.restartTimer = setTimeout(() => {
			this.restartTimer = null;
			if (!this.intentionalStop) {
				this.spawnProcess();
			}
		}, delayMs);
	}

	private killProcess(): void {
		// Cancel the stdout reader first
		if (this.stdoutReader) {
			this.stdoutReader.cancel().catch(() => {});
			this.stdoutReader = null;
		}

		if (this.process) {
			this.process.kill();
			this.process = null;
			this.buffer = Buffer.alloc(0);
		}
	}

	private cancelRestart(): void {
		if (this.restartTimer) {
			clearTimeout(this.restartTimer);
			this.restartTimer = null;
		}
	}
}

// ============================================================================
// Factory function
// ============================================================================

export function createAudioSupervisor(
	options?: AudioSupervisorOptions,
): AudioSupervisor {
	return new AudioSupervisor(options);
}
