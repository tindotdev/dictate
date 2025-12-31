import { spawn, type ChildProcess } from 'child_process';
import { EventEmitter } from 'events';
import {
  type BackoffConfig,
  createBackoffState,
  nextBackoff,
  resetBackoff,
  type BackoffState,
} from '../backoff.js';

// ============================================================================
// Audio Constants (OpenAI Realtime API requirements)
// ============================================================================

const SAMPLE_RATE = 24000;
const CHANNELS = 1;
const BYTES_PER_SAMPLE = 2; // s16 = 16-bit = 2 bytes
const FRAME_MS = 20; // 20ms frames
const FRAME_BYTES = (SAMPLE_RATE * BYTES_PER_SAMPLE * CHANNELS * FRAME_MS) / 1000; // 960 bytes

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

export type AudioSupervisorState = 'stopped' | 'starting' | 'running' | 'restarting' | 'failed';

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
  on<K extends keyof AudioSupervisorEvents>(event: K, listener: AudioSupervisorEvents[K]): this;
  emit<K extends keyof AudioSupervisorEvents>(
    event: K,
    ...args: Parameters<AudioSupervisorEvents[K]>
  ): boolean;
}

export interface AudioSupervisorOptions {
  backoff?: Partial<BackoffConfig>;
  /** Path to pw-cat binary (default: 'pw-cat') */
  pwCatPath?: string;
}

// ============================================================================
// Audio Supervisor
// ============================================================================

export class AudioSupervisor extends EventEmitter {
  private process: ChildProcess | null = null;
  private buffer: Buffer = Buffer.alloc(0);
  private state: AudioSupervisorState = 'stopped';
  private intentionalStop = false;
  private backoffState: BackoffState;
  private restartTimer: ReturnType<typeof setTimeout> | null = null;
  private pwCatPath: string;

  constructor(options: AudioSupervisorOptions = {}) {
    super();
    this.backoffState = createBackoffState(options.backoff);
    this.pwCatPath = options.pwCatPath ?? 'pw-cat';
  }

  private setState(newState: AudioSupervisorState): void {
    if (this.state !== newState) {
      const from = this.state;
      this.state = newState;
      this.emit('state_change', from, newState);
    }
  }

  getState(): AudioSupervisorState {
    return this.state;
  }

  isRunning(): boolean {
    return this.state === 'running';
  }

  /**
   * Start audio capture. If already running, this is a no-op.
   */
  start(): void {
    if (this.state === 'running' || this.state === 'starting') {
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
    if (this.state !== 'stopped') {
      this.setState('stopped');
      this.emit('stopped');
    }
  }

  private spawnProcess(): void {
    this.setState('starting');

    try {
      this.process = spawn(this.pwCatPath, [
        '--record',
        '--raw',
        `--rate=${SAMPLE_RATE}`,
        `--channels=${CHANNELS}`,
        '--format=s16',
        '-',
      ]);
    } catch (err) {
      // Spawn failed synchronously (e.g., ENOENT)
      this.emit('error', err as Error);
      this.handleExit(-1, err as Error);
      return;
    }

    this.process.stdout?.on('data', (data: Buffer) => {
      this.buffer = Buffer.concat([this.buffer, data]);

      // Emit complete frames
      while (this.buffer.length >= FRAME_BYTES) {
        const frame = this.buffer.subarray(0, FRAME_BYTES);
        this.buffer = this.buffer.subarray(FRAME_BYTES);
        this.emit('chunk', frame);
      }
    });

    this.process.stderr?.on('data', (_data: Buffer) => {
      // pw-cat outputs info to stderr, we ignore it here
      // (main.ts or consumer can handle debug logging)
    });

    this.process.on('spawn', () => {
      // Process spawned successfully
      this.setState('running');
      // Note: Don't reset backoff here - wait until we know process is stable
      // The backoff is reset on intentional stop() or can be reset manually
      this.emit('started');
    });

    this.process.on('error', (err) => {
      this.emit('error', err);
      this.handleExit(-1, err);
    });

    this.process.on('close', (code) => {
      this.handleExit(code ?? 0);
    });
  }

  private handleExit(code: number, error?: Error): void {
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
      this.setState('failed');
      this.emit(
        'failed',
        error ?? new Error(`pw-cat failed after ${this.backoffState.config.maxRetries} retries`)
      );
      return;
    }

    this.setState('restarting');
    this.emit('restarting', this.backoffState.attempt, delayMs);

    this.restartTimer = setTimeout(() => {
      this.restartTimer = null;
      if (!this.intentionalStop) {
        this.spawnProcess();
      }
    }, delayMs);
  }

  private killProcess(): void {
    if (this.process) {
      this.process.kill('SIGTERM');
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

export function createAudioSupervisor(options?: AudioSupervisorOptions): AudioSupervisor {
  return new AudioSupervisor(options);
}
