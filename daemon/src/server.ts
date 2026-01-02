import { EventEmitter } from "node:events";
import { existsSync, unlinkSync } from "node:fs";
import { chmod, mkdir, stat, unlink } from "node:fs/promises";
import * as path from "node:path";
import type { Socket as BunSocket, SocketHandler } from "bun";
import { getSocketPath } from "./cli/lib/socket-path.js";
import {
	type ClientMessage,
	ClientMessageSchema,
	DAEMON_VERSION,
	type DaemonMessage,
} from "./protocol.js";

// ============================================================================
// Types
// ============================================================================

/** Client-specific data attached to each socket */
interface ClientData {
	id: string;
	version?: string;
	buffer: string;
}

export interface Client {
	id: string;
	socket: BunSocket<ClientData>;
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
	on<K extends keyof SocketServerEvents>(
		event: K,
		listener: SocketServerEvents[K],
	): this;
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

async function ensureSocketDir(socketPath: string): Promise<void> {
	const dir = path.dirname(socketPath);
	try {
		await stat(dir);
	} catch {
		// Directory doesn't exist, create it
		await mkdir(dir, { recursive: true, mode: 0o700 });
	}
}

async function cleanupStaleSocket(socketPath: string): Promise<void> {
	try {
		// Check if socket file exists
		const stats = await stat(socketPath);

		if (!stats.isSocket()) {
			// Not a socket (regular file, etc.) - safe to remove
			await unlink(socketPath);
			return;
		}

		// It's a socket - check if something is listening
		// We do this by just removing it; if another process
		// is using it, they'll get EADDRINUSE when we try to bind
		await unlink(socketPath);
	} catch {
		// File doesn't exist or error - that's fine
	}
}

// ============================================================================
// Socket Server
// ============================================================================

/** Server instance returned by Bun.listen */
type BunServer = ReturnType<typeof Bun.listen>;

export class SocketServer extends EventEmitter {
	private server: BunServer | null = null;
	private clients: Map<string, BunSocket<ClientData>> = new Map();
	private nextClientId = 1;
	private socketPath: string | null = null;
	private _ready: Promise<void> | null = null;

	/**
	 * Promise that resolves when the server is ready to accept connections.
	 * Only relevant in standalone mode (not systemd socket activation).
	 */
	get ready(): Promise<void> {
		return this._ready ?? Promise.resolve();
	}

	/**
	 * Start the server. Supports systemd socket activation or standalone mode.
	 */
	listen(options: SocketServerOptions = {}): void {
		if (this.server) {
			return; // Already listening
		}

		// Check for systemd socket activation
		if (process.env.LISTEN_FDS === "1") {
			// fd 3 is the socket passed by systemd
			this.server = Bun.listen<ClientData>({
				fd: 3,
				socket: this.createSocketHandlers(),
			});
			return;
		}

		// Standalone mode: create socket ourselves
		this.socketPath = options.socketPath ?? getSocketPath();

		// Setup socket directory and cleanup, then start listening
		this._ready = this.setupAndListen();
	}

	private async setupAndListen(): Promise<void> {
		if (!this.socketPath) return;

		await ensureSocketDir(this.socketPath);
		await cleanupStaleSocket(this.socketPath);

		this.server = Bun.listen<ClientData>({
			unix: this.socketPath,
			socket: this.createSocketHandlers(),
		});

		// Set socket permissions (owner only)
		await chmod(this.socketPath, 0o600);
	}

	private createSocketHandlers(): SocketHandler<ClientData> {
		return {
			open: (socket) => {
				const clientId = `client_${this.nextClientId++}`;
				socket.data = {
					id: clientId,
					buffer: "",
				};

				this.clients.set(clientId, socket);
				this.emit("client_connected", clientId);
			},

			data: (socket, data) => {
				this.handleData(socket, data);
			},

			close: (socket) => {
				const clientId = socket.data.id;
				this.clients.delete(clientId);
				this.emit("client_disconnected", clientId);
			},

			error: (socket, err) => {
				const clientId = socket.data?.id ?? "unknown";
				this.emit("error", new Error(`Client ${clientId}: ${err.message}`));
			},

			drain: (_socket) => {
				// Socket ready for more data (backpressure cleared)
			},
		};
	}

	/**
	 * Stop the server and disconnect all clients.
	 */
	close(): void {
		// Disconnect all clients
		for (const socket of this.clients.values()) {
			socket.end();
		}
		this.clients.clear();

		// Close server
		if (this.server) {
			this.server.stop();
			this.server = null;
		}

		// Clean up socket file (only in standalone mode)
		if (this.socketPath) {
			try {
				if (existsSync(this.socketPath)) {
					unlinkSync(this.socketPath);
				}
			} catch {
				// Ignore cleanup errors
			}
		}
		this.socketPath = null;
		this._ready = null;
	}

	/**
	 * Send a message to a specific client.
	 */
	send(clientId: string, message: DaemonMessage): void {
		const socket = this.clients.get(clientId);
		if (socket) {
			const line = `${JSON.stringify(message)}\n`;
			socket.write(line);
		}
	}

	/**
	 * Broadcast a message to all connected clients.
	 */
	broadcast(message: DaemonMessage): void {
		const line = `${JSON.stringify(message)}\n`;
		for (const socket of this.clients.values()) {
			socket.write(line);
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

	private handleData(
		socket: BunSocket<ClientData>,
		data: Buffer | Uint8Array,
	): void {
		const clientData = socket.data;
		clientData.buffer += Buffer.from(data).toString();

		// Process complete lines
		for (
			let newlineIndex = clientData.buffer.indexOf("\n");
			newlineIndex !== -1;
			newlineIndex = clientData.buffer.indexOf("\n")
		) {
			const line = clientData.buffer.slice(0, newlineIndex);
			clientData.buffer = clientData.buffer.slice(newlineIndex + 1);

			if (!line.trim()) continue;

			try {
				const parsed = JSON.parse(line);
				const message = ClientMessageSchema.parse(parsed);

				// Handle initialize specially to track client version
				if (message.type === "initialize") {
					clientData.version = message.version;
					// Send initialized response
					this.send(clientData.id, {
						type: "initialized",
						client_id: clientData.id,
						daemon_version: DAEMON_VERSION,
					});
				}

				this.emit("client_message", clientData.id, message);
			} catch (err) {
				this.send(clientData.id, {
					type: "error",
					code: "INTERNAL_ERROR",
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
