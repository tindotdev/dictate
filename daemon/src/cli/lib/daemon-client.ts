import type { Socket as BunSocket } from "bun";
import {
	type BackoffState,
	createBackoffState,
	nextBackoff,
	resetBackoff,
} from "../../backoff.js";
import type { ClientMessage, DaemonMessage } from "../../protocol.js";
import { ensureDaemonRunning } from "./daemon-autostart.js";
import { getSocketPath } from "./socket-path.js";

// ============================================================================
// Types
// ============================================================================

export interface DaemonClientOptions {
	/** Socket path (defaults to getSocketPath()) */
	socketPath?: string;
	/** Called when a message is received from daemon */
	onMessage: (msg: DaemonMessage) => void;
	/** Called when connection is established */
	onConnect?: () => void;
	/** Called when disconnected */
	onDisconnect?: () => void;
	/** Called on error */
	onError?: (error: Error) => void;
	/** Auto-start daemon if socket missing (default: true) */
	autoStart?: boolean;
	/** Retry connection on failure (default: true) */
	reconnect?: boolean;
	/** Max reconnection attempts (default: 5, 0 = infinite) */
	maxReconnectAttempts?: number;
	/** Connection timeout in ms (default: 2000) */
	connectTimeoutMs?: number;
}

export interface DaemonClient {
	/** Connect to daemon (auto-starts if needed and enabled) */
	connect(): Promise<void>;
	/** Send a message to daemon */
	send(msg: ClientMessage): void;
	/** Disconnect from daemon */
	disconnect(): void;
	/** Check if connected */
	isConnected(): boolean;
}

/** Socket data for Bun.connect */
interface SocketData {
	buffer: string;
}

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_CONNECT_TIMEOUT_MS = 2000;
const DEFAULT_MAX_RECONNECT_ATTEMPTS = 5;

// ============================================================================
// Client implementation
// ============================================================================

export function createDaemonClient(options: DaemonClientOptions): DaemonClient {
	const socketPath = options.socketPath ?? getSocketPath();
	const autoStart = options.autoStart ?? true;
	const reconnect = options.reconnect ?? true;
	const maxReconnectAttempts =
		options.maxReconnectAttempts ?? DEFAULT_MAX_RECONNECT_ATTEMPTS;
	const connectTimeoutMs =
		options.connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS;

	let socket: BunSocket<SocketData> | null = null;
	let backoffState: BackoffState = createBackoffState({
		maxRetries: maxReconnectAttempts,
		baseDelayMs: 100,
		maxDelayMs: 5000,
	});
	let intentionalDisconnect = false;
	let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

	function handleSocketData(
		sock: BunSocket<SocketData>,
		data: Buffer | Uint8Array,
	): void {
		sock.data.buffer += Buffer.from(data).toString();

		// Process complete lines (JSONL)
		for (
			let newlineIndex = sock.data.buffer.indexOf("\n");
			newlineIndex !== -1;
			newlineIndex = sock.data.buffer.indexOf("\n")
		) {
			const line = sock.data.buffer.slice(0, newlineIndex);
			sock.data.buffer = sock.data.buffer.slice(newlineIndex + 1);

			if (line.trim()) {
				try {
					const msg = JSON.parse(line) as DaemonMessage;
					options.onMessage(msg);
				} catch {
					// Invalid JSON - ignore
				}
			}
		}
	}

	function handleConnectionFailed(reason: string): void {
		if (!reconnect) {
			options.onError?.(new Error(`Connection failed: ${reason}`));
			return;
		}

		const delayMs = nextBackoff(backoffState);
		if (delayMs === null) {
			options.onError?.(
				new Error(
					`Connection failed after ${maxReconnectAttempts} attempts: ${reason}`,
				),
			);
			return;
		}

		scheduleReconnect(delayMs);
	}

	function handleDisconnect(): void {
		socket = null;
		options.onDisconnect?.();

		if (intentionalDisconnect || !reconnect) {
			return;
		}

		const delayMs = nextBackoff(backoffState);
		if (delayMs === null) {
			options.onError?.(
				new Error(
					`Lost connection after ${maxReconnectAttempts} reconnection attempts`,
				),
			);
			return;
		}

		scheduleReconnect(delayMs);
	}

	function scheduleReconnect(delayMs: number): void {
		reconnectTimer = setTimeout(() => {
			reconnectTimer = null;
			if (!intentionalDisconnect) {
				doConnect().catch((err) => {
					options.onError?.(err);
				});
			}
		}, delayMs);
	}

	async function doConnect(): Promise<void> {
		return new Promise((resolve, reject) => {
			const timeoutId = setTimeout(() => {
				if (!socket) {
					handleConnectionFailed("Connection timeout");
					reject(new Error("Connection timeout"));
				}
			}, connectTimeoutMs);

			Bun.connect<SocketData>({
				unix: socketPath,
				socket: {
					open: (sock) => {
						clearTimeout(timeoutId);
						socket = sock;
						sock.data = { buffer: "" };
						resetBackoff(backoffState);
						options.onConnect?.();
						resolve();
					},

					data: (sock, data) => {
						handleSocketData(sock, data);
					},

					close: () => {
						socket = null;
						if (!intentionalDisconnect) {
							handleDisconnect();
						}
					},

					connectError: (_sock, error) => {
						clearTimeout(timeoutId);
						handleConnectionFailed(error.message);
						reject(error);
					},

					error: (_sock, _error) => {
						// Socket error after connection - will be followed by close
					},

					end: () => {
						// Server closed connection - will be followed by close
					},
				},
			}).catch((err) => {
				clearTimeout(timeoutId);
				handleConnectionFailed(err.message);
				reject(err);
			});
		});
	}

	const client: DaemonClient = {
		async connect(): Promise<void> {
			intentionalDisconnect = false;
			backoffState = createBackoffState({
				maxRetries: maxReconnectAttempts,
				baseDelayMs: 100,
				maxDelayMs: 5000,
			});

			// Auto-start daemon if enabled
			if (autoStart) {
				const result = await ensureDaemonRunning({ socketPath });
				if (!result.success) {
					throw new Error(
						result.hint ?? result.error ?? "Failed to start daemon",
					);
				}
			}

			// Connect to daemon
			await doConnect();
		},

		send(msg: ClientMessage): void {
			if (socket) {
				const line = JSON.stringify(msg);
				socket.write(`${line}\n`);
			}
		},

		disconnect(): void {
			intentionalDisconnect = true;

			if (reconnectTimer) {
				clearTimeout(reconnectTimer);
				reconnectTimer = null;
			}

			if (socket) {
				socket.end();
				socket = null;
			}
		},

		isConnected(): boolean {
			return socket !== null;
		},
	};

	return client;
}
