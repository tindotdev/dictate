/**
 * Multi-Client Integration Tests
 *
 * These tests verify that the daemon correctly handles multiple simultaneous
 * client connections, corresponding to TEST_CHECKLIST.md Section 4:
 *
 * 4.1 Multiple dictatectl Instances - Multiple clients can connect simultaneously
 * 4.2 Broadcast to All Clients - Status events reach all clients
 * 4.3 Client Disconnect - Daemon handles disconnections gracefully
 * 4.4 Multiple Neovim Instances - Duplicate commands handled safely
 * 5. Session Ownership - Only session owner receives transcription events
 *
 * Architecture:
 * - Real SocketServer with temp socket files
 * - Real StateMachine
 * - Mock AudioSupervisor (event emitter, no real pw-cat process)
 * - Mock NetworkSupervisor (connects to local WebSocket server)
 *
 * Run with: bun test src/__tests__/integration/multi-client.test.ts
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { EventEmitter } from "node:events";
import * as fs from "node:fs";
import * as net from "node:net";
import * as os from "node:os";
import * as path from "node:path";
import { WebSocketServer } from "ws";
import type { Config } from "../../config.js";
import type {
	ClientMessage,
	DaemonMessage,
	DaemonState,
} from "../../protocol.js";
import { createSocketServer, type SocketServer } from "../../server.js";
import {
	createStateMachine,
	type DaemonStateMachine,
} from "../../state-machine.js";
import {
	createNetworkSupervisor,
	type NetworkSupervisor,
} from "../../supervisors/network.js";

// ============================================================================
// Mock Audio Supervisor (doesn't spawn real processes)
// ============================================================================

interface MockAudioSupervisor extends EventEmitter {
	start(): void;
	stop(): void;
	isRunning(): boolean;
	getState(): string;
}

function createMockAudioSupervisor(): MockAudioSupervisor {
	const emitter = new EventEmitter() as MockAudioSupervisor;
	let running = false;

	emitter.start = () => {
		if (!running) {
			running = true;
			// Emit started asynchronously like the real supervisor
			setImmediate(() => {
				emitter.emit("started");
			});
		}
	};

	emitter.stop = () => {
		if (running) {
			running = false;
			emitter.emit("stopped");
		}
	};

	emitter.isRunning = () => running;
	emitter.getState = () => (running ? "running" : "stopped");

	return emitter;
}

// ============================================================================
// Test Harness: Wires components together like main.ts
// ============================================================================

interface TestDaemon {
	server: SocketServer;
	stateMachine: DaemonStateMachine;
	audio: MockAudioSupervisor;
	network: NetworkSupervisor;
	mockWsServer: WebSocketServer;
	socketPath: string;
	cleanup: () => void;
	getSessionOwner: () => string | null;
}

const mockConfig: Config = {
	apiKey: "test-key",
	model: "gpt-4o-mini-transcribe",
	vadThreshold: 0.5,
	vadPrefixPaddingMs: 300,
	vadSilenceDurationMs: 500,
};

async function createTestDaemon(wsPort: number): Promise<TestDaemon> {
	// Create temp directory for socket
	const testDir = fs.mkdtempSync(
		path.join(os.tmpdir(), "dictate-integration-"),
	);
	const socketPath = path.join(testDir, "dictate.sock");

	// Create mock WebSocket server
	const mockWsServer = await new Promise<WebSocketServer>((resolve) => {
		const wss = new WebSocketServer({ port: wsPort });
		wss.on("listening", () => resolve(wss));
	});

	// Create components
	const server = createSocketServer();
	const stateMachine = createStateMachine();
	const audio = createMockAudioSupervisor();
	const network = createNetworkSupervisor({
		config: mockConfig,
		wsUrl: `ws://localhost:${wsPort}`,
		backoff: { maxRetries: 1, baseDelayMs: 10, jitterFactor: 0 },
	});

	// Session ownership tracking
	let sessionOwner: string | null = null;

	// Helper: send only to session owner
	function sendToOwner(msg: DaemonMessage): void {
		if (sessionOwner) {
			server.send(sessionOwner, msg);
		}
	}

	// Wire up event handlers (replicating main.ts logic)

	// State machine -> broadcast status + clear ownership on idle
	stateMachine.on("transition", (_from, to) => {
		// Clear session ownership when returning to idle
		if (to === "idle" && sessionOwner !== null) {
			sessionOwner = null;
		}

		server.broadcast({
			type: "status",
			state: stateMachine.getState(),
			audio_ok: audio.isRunning(),
			ws_ok: network.isConnected(),
		});
	});

	// Audio supervisor -> state machine
	audio.on("started", () => {
		stateMachine.transition({ type: "AUDIO_READY" });
	});

	// Network supervisor -> state machine
	network.on("connected", () => {
		stateMachine.transition({ type: "WS_READY" });
	});

	network.on("disconnected", () => {
		// Only transition if we're in a connected state
		if (stateMachine.getState() === "listening") {
			stateMachine.transition({ type: "WS_DISCONNECTED" });
		}
	});

	// Network events -> send only to session owner
	network.on("speech_started", (itemId) => {
		sendToOwner({ type: "speech_started", item_id: itemId });
	});

	network.on("delta", (itemId, text) => {
		sendToOwner({ type: "partial_transcript", item_id: itemId, text });
	});

	network.on("completed", (itemId, transcript) => {
		sendToOwner({
			type: "final_transcript",
			item_id: itemId,
			text: transcript,
		});
		if (stateMachine.getState() === "flushing") {
			stateMachine.transition({ type: "FINAL_TRANSCRIPT_RECEIVED" });
		}
	});

	// Socket server -> handle client messages
	server.on("client_connected", (clientId) => {
		// Send current status to new client
		server.send(clientId, {
			type: "status",
			state: stateMachine.getState(),
			audio_ok: audio.isRunning(),
			ws_ok: network.isConnected(),
		});
	});

	server.on("client_disconnected", (clientId) => {
		// If the disconnected client was the session owner, force-stop the session
		if (sessionOwner === clientId) {
			// Note: sessionOwner is cleared by the transition callback when state becomes idle
			const state = stateMachine.getState();
			if (state !== "idle" && state !== "error") {
				audio.stop();
				network.disconnect();
				// Use RESET to force-return to idle from any state
				stateMachine.transition({ type: "RESET" });
			}
		}
	});

	server.on("client_message", (clientId, msg) => {
		switch (msg.type) {
			case "start_listening":
			case "start": {
				// Check session ownership
				if (sessionOwner !== null && sessionOwner !== clientId) {
					// Reject: session already active
					server.send(clientId, {
						type: "error",
						code: "SESSION_BUSY",
						message: "Another client is already dictating",
						recoverable: true,
						hint: "Wait for the other session to end",
					});
					return;
				}

				// Idempotent: if this client already owns and we're listening
				if (
					sessionOwner === clientId &&
					stateMachine.getState() === "listening"
				) {
					return;
				}

				const state = stateMachine.getState();
				if (state === "error") {
					return;
				}

				const started = stateMachine.transition({ type: "START_LISTENING" });
				if (started) {
					sessionOwner = clientId;
					network.connect();
					audio.start();
				}
				break;
			}

			case "stop_listening":
			case "stop": {
				// Only the session owner can stop listening
				if (sessionOwner !== null && sessionOwner !== clientId) {
					return; // Silently ignore non-owner stop
				}

				const state = stateMachine.getState();
				if (state === "idle") {
					return; // Already idle
				}
				stateMachine.transition({ type: "STOP_LISTENING" });
				audio.stop();
				// For tests, immediately transition to idle
				stateMachine.transition({ type: "FINAL_TRANSCRIPT_RECEIVED" });
				network.disconnect();
				break;
			}
		}
	});

	// Start server
	server.listen({ socketPath });
	await server.ready;

	const cleanup = () => {
		audio.stop();
		network.disconnect();
		server.close();
		mockWsServer.close();
		if (fs.existsSync(testDir)) {
			fs.rmSync(testDir, { recursive: true });
		}
	};

	return {
		server,
		stateMachine,
		audio,
		network,
		mockWsServer,
		socketPath,
		cleanup,
		getSessionOwner: () => sessionOwner,
	};
}

// ============================================================================
// Test Client Helpers
// ============================================================================

interface TestClient {
	socket: net.Socket;
	messages: DaemonMessage[];
	send: (msg: ClientMessage) => void;
	waitForMessage: (
		predicate: (msg: DaemonMessage) => boolean,
		timeoutMs?: number,
	) => Promise<DaemonMessage>;
	waitForState: (
		state: DaemonState,
		timeoutMs?: number,
	) => Promise<DaemonMessage>;
	getLastStatus: () => DaemonMessage | undefined;
	destroy: () => void;
}

function createTestClient(socketPath: string): Promise<TestClient> {
	return new Promise((resolve, reject) => {
		const socket = new net.Socket();
		const messages: DaemonMessage[] = [];
		let buffer = "";
		let messageCount = 0;

		socket.on("data", (data) => {
			buffer += data.toString();
			for (
				let newlineIndex = buffer.indexOf("\n");
				newlineIndex !== -1;
				newlineIndex = buffer.indexOf("\n")
			) {
				const line = buffer.slice(0, newlineIndex);
				buffer = buffer.slice(newlineIndex + 1);
				if (line.trim()) {
					try {
						const msg = JSON.parse(line) as DaemonMessage;
						messages.push(msg);
						messageCount++;
					} catch {
						// Ignore parse errors in tests
					}
				}
			}
		});

		socket.on("error", reject);

		socket.connect(socketPath, () => {
			const client: TestClient = {
				socket,
				messages,

				send(msg: ClientMessage) {
					socket.write(`${JSON.stringify(msg)}\n`);
				},

				waitForMessage(predicate, timeoutMs = 2000) {
					return new Promise((res, rej) => {
						const _startCount = messageCount;

						const check = () => {
							// Check all messages, newest first (more likely to match)
							for (let i = messages.length - 1; i >= 0; i--) {
								if (predicate(messages[i])) {
									return messages[i];
								}
							}
							return null;
						};

						// Check if already received
						const existing = check();
						if (existing) {
							res(existing);
							return;
						}

						const timeout = setTimeout(() => {
							clearInterval(interval);
							rej(
								new Error(
									`Timeout waiting for message. Messages received: ${messages.length}, looking for predicate. Last 5: ${JSON.stringify(messages.slice(-5))}`,
								),
							);
						}, timeoutMs);

						const interval = setInterval(() => {
							const found = check();
							if (found) {
								clearTimeout(timeout);
								clearInterval(interval);
								res(found);
							}
						}, 10);
					});
				},

				waitForState(state, timeoutMs = 2000) {
					return this.waitForMessage(
						(msg) => msg.type === "status" && msg.state === state,
						timeoutMs,
					);
				},

				getLastStatus() {
					for (let i = messages.length - 1; i >= 0; i--) {
						if (messages[i].type === "status") {
							return messages[i];
						}
					}
					return undefined;
				},

				destroy() {
					socket.destroy();
				},
			};

			resolve(client);
		});
	});
}

// Helper to wait for a condition
function waitFor(conditionFn: () => boolean, timeoutMs = 1000): Promise<void> {
	return new Promise((resolve, reject) => {
		const start = Date.now();
		const check = () => {
			if (conditionFn()) {
				resolve();
			} else if (Date.now() - start > timeoutMs) {
				reject(new Error("Timeout waiting for condition"));
			} else {
				setTimeout(check, 10);
			}
		};
		check();
	});
}

// ============================================================================
// Tests
// ============================================================================

describe("Multi-Client Integration", () => {
	let daemon: TestDaemon;
	let clients: TestClient[] = [];
	let wsPort = 19100; // Start port for mock WS servers

	beforeEach(async () => {
		wsPort++; // Use different port for each test
		daemon = await createTestDaemon(wsPort);
		clients = [];
	});

	afterEach(() => {
		for (const client of clients) {
			client.destroy();
		}
		clients = [];
		daemon.cleanup();
	});

	async function connectClient(): Promise<TestClient> {
		const client = await createTestClient(daemon.socketPath);
		clients.push(client);
		return client;
	}

	// ==========================================================================
	// Test 4.1: Multiple dictatectl Instances Connect
	// ==========================================================================

	describe("4.1 Multiple clients connecting", () => {
		it("accepts multiple simultaneous connections", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();
			const client3 = await connectClient();

			await waitFor(() => daemon.server.getClientCount() === 3);

			expect(daemon.server.getClientCount()).toBe(3);
			expect(daemon.server.getClientIds().length).toBe(3);

			// All should be connected
			expect(client1.socket.destroyed).toBe(false);
			expect(client2.socket.destroyed).toBe(false);
			expect(client3.socket.destroyed).toBe(false);
		});

		it("sends initial status to each connecting client", async () => {
			const client1 = await connectClient();

			// Wait for initial status
			const status1 = await client1.waitForState("idle");
			expect(status1.type).toBe("status");
			expect(status1.state).toBe("idle");

			const client2 = await connectClient();
			const status2 = await client2.waitForState("idle");
			expect(status2.type).toBe("status");
			expect(status2.state).toBe("idle");
		});

		it("assigns unique client IDs", async () => {
			const clientIds: string[] = [];
			daemon.server.on("client_connected", (id) => clientIds.push(id));

			await connectClient();
			await connectClient();
			await connectClient();

			await waitFor(() => clientIds.length === 3);

			// All IDs should be unique
			const uniqueIds = new Set(clientIds);
			expect(uniqueIds.size).toBe(3);

			// IDs should follow pattern
			for (const id of clientIds) {
				expect(id).toMatch(/^client_\d+$/);
			}
		});
	});

	// ==========================================================================
	// Test 4.2: Broadcast to All Clients
	// ==========================================================================

	describe("4.2 Broadcast status to all clients", () => {
		it("broadcasts state changes to all connected clients", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			// Wait for initial status
			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 sends start_listening
			client1.send({ type: "start_listening" });

			// Wait a bit for processing
			await new Promise((r) => setTimeout(r, 100));

			// Both clients should have received audio_starting (check in message history)
			const hasAudioStarting1 = client1.messages.some(
				(m) => m.type === "status" && m.state === "audio_starting",
			);
			const hasAudioStarting2 = client2.messages.some(
				(m) => m.type === "status" && m.state === "audio_starting",
			);

			expect(hasAudioStarting1).toBe(true);
			expect(hasAudioStarting2).toBe(true);
		});

		it("broadcasts listening state when both audio and WS are ready", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			client1.send({ type: "start_listening" });

			// Both should eventually reach listening state
			const [listening1, listening2] = await Promise.all([
				client1.waitForState("listening"),
				client2.waitForState("listening"),
			]);

			expect(listening1.state).toBe("listening");
			expect(listening1.audio_ok).toBe(true);
			expect(listening1.ws_ok).toBe(true);

			expect(listening2.state).toBe("listening");
			expect(listening2.audio_ok).toBe(true);
			expect(listening2.ws_ok).toBe(true);
		});

		it("sends transcription events only to session owner", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Simulate OpenAI sending transcription events via mock WS server
			daemon.mockWsServer.clients.forEach((ws) => {
				ws.send(
					JSON.stringify({
						type: "input_audio_buffer.speech_started",
						item_id: "test_item_123",
					}),
				);
			});

			// Only owner (client1) should receive speech_started
			const speech1 = await client1.waitForMessage(
				(m) => m.type === "speech_started",
			);
			expect(speech1.type).toBe("speech_started");
			expect((speech1 as { item_id: string }).item_id).toBe("test_item_123");

			// Client2 should NOT have received speech_started
			await new Promise((r) => setTimeout(r, 100));
			const speech2 = client2.messages.find((m) => m.type === "speech_started");
			expect(speech2).toBeUndefined();
		});

		it("sends partial transcripts only to session owner", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Simulate partial transcript
			daemon.mockWsServer.clients.forEach((ws) => {
				ws.send(
					JSON.stringify({
						type: "conversation.item.input_audio_transcription.delta",
						item_id: "test_item_123",
						delta: "hello world",
					}),
				);
			});

			// Only owner (client1) should receive partial_transcript
			const delta1 = await client1.waitForMessage(
				(m) => m.type === "partial_transcript",
			);
			expect((delta1 as { text: string }).text).toBe("hello world");

			// Client2 should NOT have received partial_transcript
			await new Promise((r) => setTimeout(r, 100));
			const delta2 = client2.messages.find(
				(m) => m.type === "partial_transcript",
			);
			expect(delta2).toBeUndefined();
		});

		it("sends final transcripts only to session owner", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Simulate final transcript
			daemon.mockWsServer.clients.forEach((ws) => {
				ws.send(
					JSON.stringify({
						type: "conversation.item.input_audio_transcription.completed",
						item_id: "test_item_123",
						transcript: "Hello, world!",
					}),
				);
			});

			// Only owner (client1) should receive final_transcript
			const final1 = await client1.waitForMessage(
				(m) => m.type === "final_transcript",
			);
			expect((final1 as { text: string }).text).toBe("Hello, world!");

			// Client2 should NOT have received final_transcript
			await new Promise((r) => setTimeout(r, 100));
			const final2 = client2.messages.find(
				(m) => m.type === "final_transcript",
			);
			expect(final2).toBeUndefined();
		});
	});

	// ==========================================================================
	// Test 4.3: Client Disconnect Handling
	// ==========================================================================

	describe("4.3 Client disconnect handling", () => {
		it("stops session when owner disconnects and notifies remaining clients", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client1 starts listening (becomes owner)
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");
			await client2.waitForState("listening");

			// Owner disconnects
			client1.destroy();

			await waitFor(() => daemon.server.getClientCount() === 1);

			// Session should be stopped (owner disconnected)
			await waitFor(() => daemon.stateMachine.getState() === "idle", 2000);

			// Client 2 should receive idle status
			const hasIdle = client2.messages.some(
				(m) => m.type === "status" && m.state === "idle",
			);
			expect(hasIdle).toBe(true);

			// Owner should be cleared
			expect(daemon.getSessionOwner()).toBeNull();
		});

		it("remaining client can start new session after owner disconnects", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 starts listening (becomes owner)
			client1.send({ type: "start_listening" });
			await client2.waitForState("listening");

			// Owner disconnects
			client1.destroy();
			await waitFor(() => daemon.server.getClientCount() === 1);

			// Wait for session to be stopped
			await waitFor(() => daemon.stateMachine.getState() === "idle", 2000);
			await client2.waitForState("idle"); // Ensure client2 received idle status

			// Record message count before starting new session
			const msgCountBefore = client2.messages.length;

			// Client 2 should now be able to start a new session
			client2.send({ type: "start_listening" });

			// Wait for a NEW listening status (not the old one)
			await waitFor(
				() =>
					client2.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "listening"),
				2000,
			);

			// Client 2 is now the owner
			expect(daemon.getSessionOwner()).not.toBeNull();
			expect(daemon.stateMachine.getState()).toBe("listening");
		});

		it("daemon accepts new connections after all clients disconnect", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// Disconnect
			client1.destroy();
			await waitFor(() => daemon.server.getClientCount() === 0);

			// New client should be able to connect
			const client2 = await connectClient();
			const status = await client2.waitForState("idle");

			expect(status.state).toBe("idle");
			expect(daemon.server.getClientCount()).toBe(1);
		});

		it("logs client disconnection", async () => {
			const disconnectedIds: string[] = [];
			daemon.server.on("client_disconnected", (id) => disconnectedIds.push(id));

			const client1 = await connectClient();
			await client1.waitForState("idle");

			const initialCount = daemon.server.getClientCount();
			expect(initialCount).toBe(1);

			client1.destroy();

			await waitFor(() => disconnectedIds.length === 1);
			expect(disconnectedIds[0]).toMatch(/^client_\d+$/);
		});
	});

	// ==========================================================================
	// Test 4.4: Duplicate Command Handling
	// ==========================================================================

	describe("4.4 Duplicate start_listening handling", () => {
		it("rejects start_listening from non-owner when session active", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 sends start_listening first
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Client 2 tries to start - should get SESSION_BUSY error
			client2.send({ type: "start_listening" });

			// Client 2 should receive error
			const error = await client2.waitForMessage((m) => m.type === "error");
			expect((error as { code: string }).code).toBe("SESSION_BUSY");

			// State machine should still be in listening state
			expect(daemon.stateMachine.getState()).toBe("listening");
		});

		it("ignores start_listening when already listening (same owner)", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Send another start_listening from same client
			const messageCountBefore = client1.messages.length;
			client1.send({ type: "start_listening" });

			// Wait a bit to ensure no state change
			await new Promise((r) => setTimeout(r, 100));

			// Should still be listening, no additional status messages
			expect(daemon.stateMachine.getState()).toBe("listening");
			// Message count should not have increased significantly
			expect(client1.messages.length).toBeLessThanOrEqual(
				messageCountBefore + 1,
			);
		});

		it("ignores stop_listening from non-owner", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 starts listening
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");
			await client2.waitForState("listening");

			// Client 2 tries to stop (not the owner) - should be ignored
			client2.send({ type: "stop_listening" });

			// Wait a bit
			await new Promise((r) => setTimeout(r, 100));

			// Should still be listening
			expect(daemon.stateMachine.getState()).toBe("listening");
		});

		it("allows owner to stop listening", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 starts listening
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");
			await client2.waitForState("listening");

			// Client 1 (owner) stops listening
			client1.send({ type: "stop_listening" });

			// Wait for state machine to reach idle
			await waitFor(() => daemon.stateMachine.getState() === "idle", 2000);

			// Verify both clients have idle in their message history
			const client1HasIdle = client1.messages.some(
				(m) => m.type === "status" && m.state === "idle",
			);
			const client2HasIdle = client2.messages.some(
				(m) => m.type === "status" && m.state === "idle",
			);

			expect(client1HasIdle).toBe(true);
			expect(client2HasIdle).toBe(true);
			expect(daemon.stateMachine.getState()).toBe("idle");
		});

		it("handles rapid start/stop commands from same client", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// Rapid toggle
			for (let i = 0; i < 3; i++) {
				client1.send({ type: "start_listening" });
				await new Promise((r) => setTimeout(r, 50));
				client1.send({ type: "stop_listening" });
				await new Promise((r) => setTimeout(r, 50));
			}

			// Should eventually settle to idle without crashing
			await new Promise((r) => setTimeout(r, 500));

			const finalState = daemon.stateMachine.getState();
			expect(["idle", "listening", "flushing"]).toContain(finalState);
		});
	});

	// ==========================================================================
	// Additional Edge Cases
	// ==========================================================================

	describe("Edge cases", () => {
		it("handles client connecting during state transition", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// Start listening
			client1.send({ type: "start_listening" });

			// Connect client2 during the transition
			const client2 = await connectClient();

			// Client2 should receive current state (whatever it is)
			const status = await client2.waitForMessage(
				(m) => m.type === "status",
				2000,
			);
			expect(status.type).toBe("status");
			expect(["idle", "audio_starting", "listening"]).toContain(status.state);
		});

		it("sends correct audio_ok and ws_ok flags", async () => {
			const client1 = await connectClient();
			const initialStatus = await client1.waitForState("idle");

			// Initially both should be false
			expect(initialStatus.audio_ok).toBe(false);
			expect(initialStatus.ws_ok).toBe(false);

			client1.send({ type: "start_listening" });

			// Wait for listening state
			const listeningStatus = await client1.waitForState("listening", 2000);
			expect(listeningStatus.audio_ok).toBe(true);
			expect(listeningStatus.ws_ok).toBe(true);
		});
	});

	// ==========================================================================
	// Test 5: Session Ownership
	// ==========================================================================

	describe("5. Session ownership", () => {
		it("tracks session owner correctly", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// No owner initially
			expect(daemon.getSessionOwner()).toBeNull();

			// Client 1 starts listening
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Client 1 should be owner
			expect(daemon.getSessionOwner()).not.toBeNull();
		});

		it("clears ownership when owner stops and returns to idle", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// Record message count before starting
			let msgCountBefore = client1.messages.length;
			client1.send({ type: "start_listening" });
			await waitFor(
				() =>
					client1.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "listening"),
				2000,
			);

			// Owner is set
			expect(daemon.getSessionOwner()).not.toBeNull();

			// Record message count before stopping
			msgCountBefore = client1.messages.length;
			client1.send({ type: "stop_listening" });
			await waitFor(
				() =>
					client1.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "idle"),
				2000,
			);

			// Owner should be cleared
			expect(daemon.getSessionOwner()).toBeNull();
		});

		it("clears ownership and stops session when owner disconnects", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 starts listening
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			expect(daemon.getSessionOwner()).not.toBeNull();

			// Owner disconnects
			client1.destroy();

			// Wait for cleanup
			await waitFor(() => daemon.stateMachine.getState() === "idle", 2000);

			// Owner should be cleared
			expect(daemon.getSessionOwner()).toBeNull();

			// Client 2 should receive idle status
			const hasIdle = client2.messages.some(
				(m) => m.type === "status" && m.state === "idle",
			);
			expect(hasIdle).toBe(true);
		});

		it("allows new session after owner disconnect", async () => {
			const client1 = await connectClient();
			const client2 = await connectClient();

			await client1.waitForState("idle");
			await client2.waitForState("idle");

			// Client 1 starts and becomes owner
			client1.send({ type: "start_listening" });
			await client1.waitForState("listening");

			// Client 1 disconnects
			client1.destroy();
			await waitFor(() => daemon.stateMachine.getState() === "idle", 2000);
			await client2.waitForState("idle"); // Ensure client2 received idle

			// Record message count before starting new session
			const msgCountBefore = client2.messages.length;

			// Client 2 should now be able to start
			client2.send({ type: "start_listening" });

			// Wait for NEW listening status
			await waitFor(
				() =>
					client2.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "listening"),
				2000,
			);

			// Client 2 is now owner
			expect(daemon.getSessionOwner()).not.toBeNull();
			expect(daemon.stateMachine.getState()).toBe("listening");
		});

		it("allows same client to start new session after stopping", async () => {
			const client1 = await connectClient();
			await client1.waitForState("idle");

			// First session
			let msgCountBefore = client1.messages.length;
			client1.send({ type: "start_listening" });
			await waitFor(
				() =>
					client1.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "listening"),
				2000,
			);

			// Record message count before stopping
			msgCountBefore = client1.messages.length;
			client1.send({ type: "stop_listening" });
			await waitFor(
				() =>
					client1.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "idle"),
				2000,
			);

			expect(daemon.getSessionOwner()).toBeNull();

			// Second session
			msgCountBefore = client1.messages.length;
			client1.send({ type: "start_listening" });
			await waitFor(
				() =>
					client1.messages
						.slice(msgCountBefore)
						.some((m) => m.type === "status" && m.state === "listening"),
				2000,
			);

			expect(daemon.getSessionOwner()).not.toBeNull();
			expect(daemon.stateMachine.getState()).toBe("listening");
		});
	});
});
