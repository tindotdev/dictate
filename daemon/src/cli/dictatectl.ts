#!/usr/bin/env bun
import * as net from 'net';
import * as fs from 'fs';
import * as path from 'path';
import * as readline from 'readline';
import {
  createBackoffState,
  nextBackoff,
  resetBackoff,
  type BackoffState,
} from '../backoff.js';
import type { DictatectlMessage } from '../protocol.js';

// ============================================================================
// Constants
// ============================================================================

const MAX_RECONNECT_ATTEMPTS = 5;
const CONNECT_TIMEOUT_MS = 2000;

// ============================================================================
// Socket path discovery
// ============================================================================

function getSocketPath(): string {
  const xdgRuntime = process.env.XDG_RUNTIME_DIR;
  if (xdgRuntime) {
    return path.join(xdgRuntime, 'dictate', 'dictate.sock');
  }
  // Fallback
  return path.join(process.env.HOME ?? '/tmp', '.local', 'state', 'dictate', 'dictate.sock');
}

// ============================================================================
// Output helpers
// ============================================================================

function emit(msg: DictatectlMessage): void {
  const line = JSON.stringify(msg);
  process.stdout.write(line + '\n');
}

function emitStatus(state: 'connecting' | 'connected' | 'reconnecting'): void {
  emit({ type: 'status', state });
}

function emitDaemonUnavailable(hint?: string): void {
  emit({
    type: 'error',
    code: 'DAEMON_UNAVAILABLE',
    message: 'Cannot connect to dictate daemon',
    recoverable: false,
    hint: hint ?? 'Run: systemctl --user enable --now dictate.service',
  });
}

// ============================================================================
// Connection handling
// ============================================================================

class DictatectlBridge {
  private socket: net.Socket | null = null;
  private backoffState: BackoffState;
  private socketPath: string;
  private intentionalDisconnect = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private stdinReader: readline.Interface | null = null;
  private socketBuffer = '';

  constructor() {
    this.socketPath = getSocketPath();
    this.backoffState = createBackoffState({
      maxRetries: MAX_RECONNECT_ATTEMPTS,
      baseDelayMs: 100,
      maxDelayMs: 5000,
    });
  }

  async start(): Promise<void> {
    // Check if socket file exists first
    if (!fs.existsSync(this.socketPath)) {
      const dir = path.dirname(this.socketPath);
      if (!fs.existsSync(dir)) {
        emitDaemonUnavailable('Socket directory does not exist. Run: systemctl --user enable --now dictate.service');
      } else {
        emitDaemonUnavailable('Socket file does not exist. Run: systemctl --user enable --now dictate.service');
      }
      process.exit(1);
    }

    // Setup stdin handling
    this.setupStdin();

    // Setup signal handlers
    process.on('SIGTERM', () => this.shutdown());
    process.on('SIGINT', () => this.shutdown());

    // Initial connection
    await this.connect();
  }

  private setupStdin(): void {
    this.stdinReader = readline.createInterface({
      input: process.stdin,
      terminal: false,
    });

    this.stdinReader.on('line', (line) => {
      if (!line.trim()) return;

      // Forward to socket if connected
      if (this.socket && !this.socket.destroyed) {
        this.socket.write(line + '\n');
      }
    });

    this.stdinReader.on('close', () => {
      // stdin closed (e.g., parent process died)
      this.shutdown();
    });
  }

  private connect(): Promise<void> {
    return new Promise((resolve) => {
      emitStatus('connecting');

      this.socket = new net.Socket();
      let connected = false;

      // Connection timeout
      const timeoutId = setTimeout(() => {
        if (!connected) {
          this.socket?.destroy();
          this.handleConnectionFailed('Connection timeout');
        }
      }, CONNECT_TIMEOUT_MS);

      this.socket.on('connect', () => {
        connected = true;
        clearTimeout(timeoutId);
        resetBackoff(this.backoffState);
        emitStatus('connected');
        resolve();
      });

      this.socket.on('data', (data: Buffer) => {
        this.handleSocketData(data);
      });

      this.socket.on('close', () => {
        if (!this.intentionalDisconnect) {
          this.handleDisconnect();
        }
      });

      this.socket.on('error', (err) => {
        if (!connected) {
          clearTimeout(timeoutId);
          this.handleConnectionFailed(err.message);
        }
        // If already connected, error is followed by close event
      });

      this.socket.connect(this.socketPath);
    });
  }

  private handleSocketData(data: Buffer): void {
    this.socketBuffer += data.toString();

    // Process complete lines (JSONL)
    let newlineIndex: number;
    while ((newlineIndex = this.socketBuffer.indexOf('\n')) !== -1) {
      const line = this.socketBuffer.slice(0, newlineIndex);
      this.socketBuffer = this.socketBuffer.slice(newlineIndex + 1);

      if (line.trim()) {
        // Forward to stdout
        process.stdout.write(line + '\n');
      }
    }
  }

  private handleConnectionFailed(reason: string): void {
    const delayMs = nextBackoff(this.backoffState);

    if (delayMs === null) {
      // Max retries exceeded
      emitDaemonUnavailable(`Connection failed after ${MAX_RECONNECT_ATTEMPTS} attempts: ${reason}`);
      process.exit(1);
    }

    this.scheduleReconnect(delayMs);
  }

  private handleDisconnect(): void {
    const delayMs = nextBackoff(this.backoffState);

    if (delayMs === null) {
      // Max retries exceeded
      emitDaemonUnavailable(`Lost connection after ${MAX_RECONNECT_ATTEMPTS} reconnection attempts`);
      process.exit(1);
    }

    this.scheduleReconnect(delayMs);
  }

  private scheduleReconnect(delayMs: number): void {
    emitStatus('reconnecting');

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

    if (this.stdinReader) {
      this.stdinReader.close();
      this.stdinReader = null;
    }

    if (this.socket) {
      this.socket.destroy();
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
