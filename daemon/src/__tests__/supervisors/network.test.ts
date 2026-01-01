import { afterEach, describe, expect, it, mock } from "bun:test";
import { WebSocketServer } from "ws";
import type { Config } from "../../config.js";
import {
	createNetworkSupervisor,
	NetworkSupervisor,
	type NetworkSupervisorState,
} from "../../supervisors/network.js";

// Mock config for testing
const mockConfig: Config = {
	apiKey: "test-key",
	model: "gpt-4o-mini-transcribe",
	vadThreshold: 0.5,
	vadPrefixPaddingMs: 300,
	vadSilenceDurationMs: 500,
};

describe("NetworkSupervisor", () => {
	let wss: WebSocketServer | null = null;
	let supervisors: NetworkSupervisor[] = [];

	afterEach(() => {
		// Clean up supervisors
		for (const sup of supervisors) {
			sup.disconnect();
		}
		supervisors = [];

		// Clean up WebSocket server
		if (wss) {
			wss.close();
			wss = null;
		}
	});

	function createTestServer(port: number): Promise<WebSocketServer> {
		return new Promise((resolve) => {
			const server = new WebSocketServer({ port });
			server.on("listening", () => resolve(server));
		});
	}

	function createTestSupervisor(
		port: number,
		backoffOptions?: object,
	): NetworkSupervisor {
		const sup = createNetworkSupervisor({
			config: mockConfig,
			wsUrl: `ws://localhost:${port}`,
			backoff: {
				maxRetries: 3,
				baseDelayMs: 10,
				jitterFactor: 0,
				...backoffOptions,
			},
		});
		supervisors.push(sup);
		return sup;
	}

	describe("initial state", () => {
		it("starts in disconnected state", () => {
			const supervisor = createNetworkSupervisor({
				config: mockConfig,
				wsUrl: "ws://localhost:9999",
			});
			supervisors.push(supervisor);

			expect(supervisor.getState()).toBe("disconnected");
			expect(supervisor.isConnected()).toBe(false);
		});
	});

	describe("connect/disconnect lifecycle", () => {
		it("transitions to connecting on connect", async () => {
			const port = 9100;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const states: NetworkSupervisorState[] = [];
			supervisor.on("state_change", (_from, to) => states.push(to));

			supervisor.connect();

			expect(states).toContain("connecting");
		});

		it("transitions to connected when WebSocket opens", async () => {
			const port = 9101;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const connectedHandler = mock(() => {});
			supervisor.on("connected", connectedHandler);

			supervisor.connect();

			// Wait for connection
			await new Promise((r) => setTimeout(r, 100));

			expect(supervisor.getState()).toBe("connected");
			expect(supervisor.isConnected()).toBe(true);
			expect(connectedHandler).toHaveBeenCalled();
		});

		it("disconnect transitions to disconnected", async () => {
			const port = 9102;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			const disconnectedHandler = mock(() => {});
			supervisor.on("disconnected", disconnectedHandler);

			supervisor.disconnect();

			expect(supervisor.getState()).toBe("disconnected");
			expect(disconnectedHandler).toHaveBeenCalled();
		});

		it("multiple connects are no-op when already connecting/connected", async () => {
			const port = 9103;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			let connectingCount = 0;
			supervisor.on("state_change", (_from, to) => {
				if (to === "connecting") connectingCount++;
			});

			supervisor.connect();
			supervisor.connect();
			supervisor.connect();

			await new Promise((r) => setTimeout(r, 100));

			expect(connectingCount).toBe(1);
		});
	});

	describe("reconnection behavior", () => {
		it("reconnects when server closes connection", async () => {
			const port = 9104;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const reconnectingHandler = mock(() => {});
			supervisor.on("reconnecting", reconnectingHandler);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			// Close server to trigger reconnection
			for (const client of wss.clients) {
				client.close();
			}

			await new Promise((r) => setTimeout(r, 100));

			expect(reconnectingHandler).toHaveBeenCalled();
		});

		it("emits failed after max retries with no server", async () => {
			// Use a port with no server
			const supervisor = createNetworkSupervisor({
				config: mockConfig,
				wsUrl: "ws://localhost:9199", // No server here
				backoff: { maxRetries: 2, baseDelayMs: 10, jitterFactor: 0 },
			});
			supervisors.push(supervisor);

			const failedHandler = mock((_err: Error) => {});
			supervisor.on("failed", failedHandler);

			// Must listen to error events to prevent unhandled error throws
			supervisor.on("error", () => {});

			supervisor.connect();

			// Wait for retries to exhaust (initial + 2 retries with delays)
			await new Promise((r) => setTimeout(r, 1000));

			expect(failedHandler).toHaveBeenCalled();
			expect(supervisor.getState()).toBe("failed");
		});

		it("intentional disconnect prevents reconnection", async () => {
			const port = 9105;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port, { baseDelayMs: 200 });

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			const reconnectingHandler = mock(() => {});
			supervisor.on("reconnecting", reconnectingHandler);

			// Close server and immediately disconnect
			for (const client of wss.clients) {
				client.close();
			}
			supervisor.disconnect();

			await new Promise((r) => setTimeout(r, 300));

			expect(reconnectingHandler).not.toHaveBeenCalled();
			expect(supervisor.getState()).toBe("disconnected");
		});
	});

	describe("event handling", () => {
		it("emits speech_started event", async () => {
			const port = 9106;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const speechStartedHandler = mock((_itemId: string) => {});
			supervisor.on("speech_started", speechStartedHandler);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			// Server sends speech_started event
			wss.clients.forEach((client) => {
				client.send(
					JSON.stringify({
						type: "input_audio_buffer.speech_started",
						item_id: "test_item_1",
					}),
				);
			});

			await new Promise((r) => setTimeout(r, 50));

			expect(speechStartedHandler).toHaveBeenCalledWith("test_item_1");
		});

		it("emits delta event", async () => {
			const port = 9107;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const deltaHandler = mock((_itemId: string, _text: string) => {});
			supervisor.on("delta", deltaHandler);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			wss.clients.forEach((client) => {
				client.send(
					JSON.stringify({
						type: "conversation.item.input_audio_transcription.delta",
						item_id: "test_item_1",
						delta: "hello",
					}),
				);
			});

			await new Promise((r) => setTimeout(r, 50));

			expect(deltaHandler).toHaveBeenCalledWith("test_item_1", "hello");
		});

		it("emits completed event", async () => {
			const port = 9108;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const completedHandler = mock(
				(_itemId: string, _transcript: string) => {},
			);
			supervisor.on("completed", completedHandler);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			wss.clients.forEach((client) => {
				client.send(
					JSON.stringify({
						type: "conversation.item.input_audio_transcription.completed",
						item_id: "test_item_1",
						transcript: "Hello, world!",
					}),
				);
			});

			await new Promise((r) => setTimeout(r, 50));

			expect(completedHandler).toHaveBeenCalledWith(
				"test_item_1",
				"Hello, world!",
			);
		});

		it("emits error event on API error", async () => {
			const port = 9109;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			const errorHandler = mock((_err: Error) => {});
			supervisor.on("error", errorHandler);

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			wss.clients.forEach((client) => {
				client.send(
					JSON.stringify({
						type: "error",
						error: {
							type: "invalid_request",
							message: "Invalid audio format",
						},
					}),
				);
			});

			await new Promise((r) => setTimeout(r, 50));

			expect(errorHandler).toHaveBeenCalled();
		});
	});

	describe("sendAudio", () => {
		it("sends audio when connected", async () => {
			const port = 9110;
			wss = await createTestServer(port);
			const supervisor = createTestSupervisor(port);

			let receivedMessage: object | null = null;
			wss.on("connection", (ws) => {
				ws.on("message", (data) => {
					const msg = JSON.parse(data.toString());
					if (msg.type === "input_audio_buffer.append") {
						receivedMessage = msg;
					}
				});
			});

			supervisor.connect();
			await new Promise((r) => setTimeout(r, 100));

			supervisor.sendAudio("dGVzdCBhdWRpbw==");

			await new Promise((r) => setTimeout(r, 50));

			expect(receivedMessage).not.toBeNull();
			expect((receivedMessage as { audio: string }).audio).toBe(
				"dGVzdCBhdWRpbw==",
			);
		});

		it("does not send audio when disconnected", () => {
			const supervisor = createNetworkSupervisor({
				config: mockConfig,
				wsUrl: "ws://localhost:9999",
			});
			supervisors.push(supervisor);

			// Should not throw
			supervisor.sendAudio("dGVzdCBhdWRpbw==");

			expect(supervisor.isConnected()).toBe(false);
		});
	});

	describe("factory function", () => {
		it("creates supervisor with config", () => {
			const supervisor = createNetworkSupervisor({
				config: mockConfig,
				wsUrl: "ws://localhost:9999",
			});
			supervisors.push(supervisor);

			expect(supervisor).toBeInstanceOf(NetworkSupervisor);
		});
	});
});
