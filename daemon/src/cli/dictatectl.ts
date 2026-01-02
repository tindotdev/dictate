import type { Socket as BunSocket } from "bun";
import {
	type BackoffState,
	createBackoffState,
	nextBackoff,
	resetBackoff,
} from "../backoff.js";
import type { DictatectlMessage } from "../protocol.js";
import { ensureDaemonRunning } from "./lib/daemon-autostart.js";
import { getSocketPath } from "./lib/socket-path.js";

// ============================================================================
// Constants
// ============================================================================

const MAX_RECONNECT_ATTEMPTS = 5;
const CONNECT_TIMEOUT_MS = 2000;

// ============================================================================
// Output helpers
// ============================================================================

function emit(msg: DictatectlMessage): void {
	const line = JSON.stringify(msg);
	process.stdout.write(`${line}\n`);
}

function emitStatus(state: "connecting" | "connected" | "reconnecting"): void {
	emit({ type: "status", state });
}

function emitDaemonUnavailable(hint?: string): void {
	emit({
		type: "error",
		code: "DAEMON_UNAVAILABLE",
		message: "Cannot connect to dictate daemon",
		recoverable: false,
		hint:
			hint ??
			"Failed to auto-start daemon. Check logs or run manually: dictated",
	});
}

// ============================================================================
// Connection handling
// ============================================================================

/** Socket data for Bun.connect */
interface SocketData {
	buffer: string;
}

class DictatectlBridge {
	private socket: BunSocket<SocketData> | null = null;
	private backoffState: BackoffState;
	private socketPath: string;
	private intentionalDisconnect = false;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private stdinAbort: AbortController | null = null;

	constructor() {
		this.socketPath = getSocketPath();
		this.backoffState = createBackoffState({
			maxRetries: MAX_RECONNECT_ATTEMPTS,
			baseDelayMs: 100,
			maxDelayMs: 5000,
		});
	}

	async start(): Promise<void> {
		// Ensure daemon is running (auto-start if needed)
		const result = await ensureDaemonRunning({ socketPath: this.socketPath });
		if (!result.success) {
			emitDaemonUnavailable(result.hint ?? result.error);
			process.exit(1);
		}

		// Setup stdin handling
		this.setupStdin();

		// Setup signal handlers
		process.on("SIGTERM", () => this.shutdown());
		process.on("SIGINT", () => this.shutdown());

		// Initial connection
		await this.connect();
	}

	private setupStdin(): void {
		this.stdinAbort = new AbortController();
		this.readStdin(this.stdinAbort.signal);
	}

	private async readStdin(signal: AbortSignal): Promise<void> {
		const decoder = new TextDecoder();
		let buffer = "";

		try {
			for await (const chunk of Bun.stdin.stream()) {
				if (signal.aborted) break;

				buffer += decoder.decode(chunk, { stream: true });

				// Process complete lines
				for (
					let newlineIndex = buffer.indexOf("\n");
					newlineIndex !== -1;
					newlineIndex = buffer.indexOf("\n")
				) {
					const line = buffer.slice(0, newlineIndex);
					buffer = buffer.slice(newlineIndex + 1);

					if (line.trim()) {
						// Forward to socket if connected
						if (this.socket) {
							this.socket.write(`${line}\n`);
						}
					}
				}
			}
		} catch {
			// Stream error or aborted - ignore
		}

		// stdin closed (e.g., parent process died)
		if (!signal.aborted) {
			this.shutdown();
		}
	}

	private async connect(): Promise<void> {
		emitStatus("connecting");

		return new Promise((resolve) => {
			// Connection timeout
			const timeoutId = setTimeout(() => {
				if (!this.socket) {
					this.handleConnectionFailed("Connection timeout");
				}
			}, CONNECT_TIMEOUT_MS);

			Bun.connect<SocketData>({
				unix: this.socketPath,
				socket: {
					open: (socket) => {
						clearTimeout(timeoutId);
						this.socket = socket;
						socket.data = { buffer: "" };
						resetBackoff(this.backoffState);
						emitStatus("connected");
						resolve();
					},

					data: (socket, data) => {
						this.handleSocketData(socket, data);
					},

					close: () => {
						this.socket = null;
						if (!this.intentionalDisconnect) {
							this.handleDisconnect();
						}
					},

					connectError: (_socket, error) => {
						clearTimeout(timeoutId);
						this.handleConnectionFailed(error.message);
					},

					error: (_socket, _error) => {
						// Socket error after connection - will be followed by close
					},

					end: () => {
						// Server closed connection - will be followed by close
					},
				},
			}).catch((err) => {
				clearTimeout(timeoutId);
				this.handleConnectionFailed(err.message);
			});
		});
	}

	private handleSocketData(
		socket: BunSocket<SocketData>,
		data: Buffer | Uint8Array,
	): void {
		socket.data.buffer += Buffer.from(data).toString();

		// Process complete lines (JSONL)
		for (
			let newlineIndex = socket.data.buffer.indexOf("\n");
			newlineIndex !== -1;
			newlineIndex = socket.data.buffer.indexOf("\n")
		) {
			const line = socket.data.buffer.slice(0, newlineIndex);
			socket.data.buffer = socket.data.buffer.slice(newlineIndex + 1);

			if (line.trim()) {
				// Forward to stdout
				process.stdout.write(`${line}\n`);
			}
		}
	}

	private handleConnectionFailed(reason: string): void {
		const delayMs = nextBackoff(this.backoffState);

		if (delayMs === null) {
			// Max retries exceeded
			emitDaemonUnavailable(
				`Connection failed after ${MAX_RECONNECT_ATTEMPTS} attempts: ${reason}. Daemon may have crashed.`,
			);
			process.exit(1);
		}

		this.scheduleReconnect(delayMs);
	}

	private handleDisconnect(): void {
		const delayMs = nextBackoff(this.backoffState);

		if (delayMs === null) {
			// Max retries exceeded
			emitDaemonUnavailable(
				`Lost connection after ${MAX_RECONNECT_ATTEMPTS} reconnection attempts. Daemon may have exited.`,
			);
			process.exit(1);
		}

		this.scheduleReconnect(delayMs);
	}

	private scheduleReconnect(delayMs: number): void {
		emitStatus("reconnecting");

		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			if (!this.intentionalDisconnect) {
				this.connect();
			}
		}, delayMs);
	}

	private shutdown(): void {
		this.intentionalDisconnect = true;

		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}

		if (this.stdinAbort) {
			this.stdinAbort.abort();
			this.stdinAbort = null;
		}

		if (this.socket) {
			this.socket.end();
			this.socket = null;
		}

		process.exit(0);
	}
}

// ============================================================================
// Main
// ============================================================================

const bridge = new DictatectlBridge();
bridge.start().catch((err) => {
	console.error(`dictatectl error: ${err.message}`);
	process.exit(1);
});
