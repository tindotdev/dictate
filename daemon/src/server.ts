import * as net from 'net';
import * as fs from 'fs';
import * as path from 'path';
import { EventEmitter } from 'events';
import {
  ClientMessageSchema,
  type ClientMessage,
  type DaemonMessage,
  DAEMON_VERSION,
} from './protocol.js';

// ============================================================================
// Types
// ============================================================================

export interface Client {
  id: string;
  socket: net.Socket;
  version?: string;
  buffer: string;
}

export interface SocketServerEvents {
  client_connected: (clientId: string) => void;
  client_disconnected: (clientId: string) => void;
  client_message: (clientId: string, message: ClientMessage) => void;
  error: (error: Error) => void;
}

export declare interface SocketServer {
  on<K extends keyof SocketServerEvents>(event: K, listener: SocketServerEvents[K]): this;
  emit<K extends keyof SocketServerEvents>(
    event: K,
    ...args: Parameters<SocketServerEvents[K]>
  ): boolean;
}

export interface SocketServerOptions {
  /** Socket path (ignored if using systemd socket activation) */
  socketPath?: string;
}

// ============================================================================
// Socket Path Helpers
// ============================================================================

export function getDefaultSocketPath(): string {
  const xdgRuntime = process.env.XDG_RUNTIME_DIR;
  if (xdgRuntime) {
    return path.join(xdgRuntime, 'say', 'say.sock');
  }
  // Fallback
  return path.join(process.env.HOME ?? '/tmp', '.local', 'state', 'say', 'say.sock');
}

function ensureSocketDir(socketPath: string): void {
  const dir = path.dirname(socketPath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
}

function cleanupStaleSocket(socketPath: string): void {
  if (fs.existsSync(socketPath)) {
    // Check if it's a socket file
    const stats = fs.statSync(socketPath);
    if (!stats.isSocket()) {
      // Not a socket (regular file, etc.) - safe to remove
      fs.unlinkSync(socketPath);
      return;
    }

    // It's a socket - check if something is listening
    // We do this synchronously by just removing it; if another process
    // is using it, they'll get EADDRINUSE when we try to bind
    fs.unlinkSync(socketPath);
  }
}

// ============================================================================
// Socket Server
// ============================================================================

export class SocketServer extends EventEmitter {
  private server: net.Server | null = null;
  private clients: Map<string, Client> = new Map();
  private nextClientId = 1;
  private socketPath: string | null = null;

  constructor() {
    super();
  }

  /**
   * Start the server. Supports systemd socket activation or standalone mode.
   */
  listen(options: SocketServerOptions = {}): void {
    if (this.server) {
      return; // Already listening
    }

    this.server = net.createServer((socket) => this.handleConnection(socket));

    this.server.on('error', (err) => {
      this.emit('error', err);
    });

    // Check for systemd socket activation
    if (process.env.LISTEN_FDS === '1') {
      // fd 3 is the socket passed by systemd
      this.server.listen({ fd: 3 });
      return;
    }

    // Standalone mode: create socket ourselves
    this.socketPath = options.socketPath ?? getDefaultSocketPath();
    ensureSocketDir(this.socketPath);
    cleanupStaleSocket(this.socketPath);

    this.server.listen(this.socketPath);

    // Set socket permissions (owner only)
    fs.chmodSync(this.socketPath, 0o600);
  }

  /**
   * Stop the server and disconnect all clients.
   */
  close(): void {
    // Disconnect all clients
    for (const client of this.clients.values()) {
      client.socket.destroy();
    }
    this.clients.clear();

    // Close server
    if (this.server) {
      this.server.close();
      this.server = null;
    }

    // Clean up socket file (only in standalone mode)
    if (this.socketPath && fs.existsSync(this.socketPath)) {
      fs.unlinkSync(this.socketPath);
    }
    this.socketPath = null;
  }

  /**
   * Send a message to a specific client.
   */
  send(clientId: string, message: DaemonMessage): void {
    const client = this.clients.get(clientId);
    if (client && !client.socket.destroyed) {
      const line = JSON.stringify(message) + '\n';
      client.socket.write(line);
    }
  }

  /**
   * Broadcast a message to all connected clients.
   */
  broadcast(message: DaemonMessage): void {
    const line = JSON.stringify(message) + '\n';
    for (const client of this.clients.values()) {
      if (!client.socket.destroyed) {
        client.socket.write(line);
      }
    }
  }

  /**
   * Get the number of connected clients.
   */
  getClientCount(): number {
    return this.clients.size;
  }

  /**
   * Get all client IDs.
   */
  getClientIds(): string[] {
    return Array.from(this.clients.keys());
  }

  private handleConnection(socket: net.Socket): void {
    const clientId = `client_${this.nextClientId++}`;
    const client: Client = {
      id: clientId,
      socket,
      buffer: '',
    };

    this.clients.set(clientId, client);
    this.emit('client_connected', clientId);

    socket.on('data', (data) => {
      this.handleData(client, data);
    });

    socket.on('close', () => {
      this.clients.delete(clientId);
      this.emit('client_disconnected', clientId);
    });

    socket.on('error', (err) => {
      // Log but don't crash on client errors
      this.emit('error', new Error(`Client ${clientId}: ${err.message}`));
    });
  }

  private handleData(client: Client, data: Buffer): void {
    client.buffer += data.toString();

    // Process complete lines
    let newlineIndex: number;
    while ((newlineIndex = client.buffer.indexOf('\n')) !== -1) {
      const line = client.buffer.slice(0, newlineIndex);
      client.buffer = client.buffer.slice(newlineIndex + 1);

      if (!line.trim()) continue;

      try {
        const parsed = JSON.parse(line);
        const message = ClientMessageSchema.parse(parsed);

        // Handle initialize specially to track client version
        if (message.type === 'initialize') {
          client.version = message.version;
          // Send initialized response
          this.send(client.id, {
            type: 'initialized',
            client_id: client.id,
            daemon_version: DAEMON_VERSION,
          });
        }

        this.emit('client_message', client.id, message);
      } catch (err) {
        this.send(client.id, {
          type: 'error',
          code: 'INTERNAL_ERROR',
          message: `Invalid message: ${(err as Error).message}`,
          recoverable: true,
        });
      }
    }
  }
}

// ============================================================================
// Factory function
// ============================================================================

export function createSocketServer(): SocketServer {
  return new SocketServer();
}
