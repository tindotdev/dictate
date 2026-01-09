/**
 * CLI Signal Handling Tests
 *
 * Tests Ctrl+C (SIGINT) behavior and graceful shutdown.
 * Verifies that the CLI:
 * 1. Sends stop_listening when SIGINT received
 * 2. Waits for final transcript before exiting
 * 3. Exits cleanly with code 0
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import * as fs from "node:fs";
import * as net from "node:net";
import * as os from "node:os";
import * as path from "node:path";
import type { ClientMessage, DaemonMessage } from "../../protocol.js";

// ============================================================================
// Mock Daemon Server (similar to cli-integration.test.ts)
// ============================================================================

interface MockDaemon {
	socketPath: string;
	server: net.Server;
	clients: net.Socket[];
	receivedMessages: ClientMessage[];
	sendToAll: (msg: DaemonMessage) => void;
	waitForMessage: (
		type: string,
		timeoutMs?: number,
	) => Promise<ClientMessage | null>;
	cleanup: () => Promise<void>;
}

async function createMockDaemon(): Promise<MockDaemon> {
	const testDir = fs.mkdtempSync(
		path.join(os.tmpdir(), "dictate-signal-test-"),
	);
	const socketPath = path.join(testDir, "dictate.sock");

	const clients: net.Socket[] = [];
	const receivedMessages: ClientMessage[] = [];
	const messageListeners: Array<(msg: ClientMessage) => void> = [];

	const server = net.createServer((socket) => {
		clients.push(socket);

		let buffer = "";
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
						const msg = JSON.parse(line) as ClientMessage;
						receivedMessages.push(msg);
						// Notify listeners
						for (const listener of messageListeners) {
							listener(msg);
						}

						// Auto-respond to initialize with initialized
						if (msg.type === "initialize") {
							socket.write(
								`${JSON.stringify({
									type: "initialized",
									client_id: "test_client_1",
									daemon_version: "0.2.0",
								} satisfies DaemonMessage)}\n`,
							);
						}
					} catch {
						// Ignore parse errors
					}
				}
			}
		});

		socket.on("close", () => {
			const idx = clients.indexOf(socket);
			if (idx !== -1) clients.splice(idx, 1);
		});

		// Send initial status on connect
		socket.write(
			`${JSON.stringify({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			} satisfies DaemonMessage)}\n`,
		);
	});

	await new Promise<void>((resolve) => {
		server.listen(socketPath, resolve);
	});

	const sendToAll = (msg: DaemonMessage) => {
		const line = `${JSON.stringify(msg)}\n`;
		for (const client of clients) {
			if (!client.destroyed && client.writable) {
				client.write(line);
			}
		}
	};

	const waitForMessage = (
		type: string,
		timeoutMs = 2000,
	): Promise<ClientMessage | null> => {
		return new Promise((resolve) => {
			// Check if already received
			const existing = receivedMessages.find((m) => m.type === type);
			if (existing) {
				resolve(existing);
				return;
			}

			const timeout = setTimeout(() => {
				const idx = messageListeners.indexOf(listener);
				if (idx !== -1) messageListeners.splice(idx, 1);
				resolve(null);
			}, timeoutMs);

			const listener = (msg: ClientMessage) => {
				if (msg.type === type) {
					clearTimeout(timeout);
					const idx = messageListeners.indexOf(listener);
					if (idx !== -1) messageListeners.splice(idx, 1);
					resolve(msg);
				}
			};

			messageListeners.push(listener);
		});
	};

	const cleanup = async () => {
		for (const client of clients) {
			client.destroy();
		}
		await new Promise<void>((resolve) => {
			server.close(() => resolve());
		});
		if (fs.existsSync(testDir)) {
			fs.rmSync(testDir, { recursive: true });
		}
	};

	return {
		socketPath,
		server,
		clients,
		receivedMessages,
		sendToAll,
		waitForMessage,
		cleanup,
	};
}

// ============================================================================
// Tests
// ============================================================================

describe("CLI Signal Handling", () => {
	let daemon: MockDaemon;

	beforeEach(async () => {
		daemon = await createMockDaemon();
	});

	afterEach(async () => {
		await daemon.cleanup();
	});

	describe("SIGINT (Ctrl+C) handling", () => {
		it("sends stop_listening when SIGINT received", async () => {
			const cliPath = path.join(
				import.meta.dir,
				"..",
				"..",
				"cli",
				"dictate.ts",
			);

			const proc = Bun.spawn(["bun", cliPath, "--verbose", "--no-clipboard"], {
				stdout: "pipe",
				stderr: "pipe",
				env: {
					...process.env,
					DICTATE_SOCKET_PATH: daemon.socketPath,
				},
			});

			// Wait for connection and start_listening
			await daemon.waitForMessage("start_listening", 2000);

			// Transition to listening state
			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});

			await new Promise((r) => setTimeout(r, 100));

			// Send SIGINT
			proc.kill("SIGINT");

			// Wait for stop_listening message
			const stopMsg = await daemon.waitForMessage("stop_listening", 2000);
			expect(stopMsg).not.toBeNull();
			expect(stopMsg?.type).toBe("stop_listening");

			// Send final transcript and idle status to allow clean exit
			daemon.sendToAll({
				type: "final_transcript",
				item_id: "final",
				text: "test",
			});
			daemon.sendToAll({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});

			await proc.exited;
		});

		it("outputs stopping message on SIGINT with --verbose", async () => {
			const cliPath = path.join(
				import.meta.dir,
				"..",
				"..",
				"cli",
				"dictate.ts",
			);

			const proc = Bun.spawn(["bun", cliPath, "--verbose", "--no-clipboard"], {
				stdout: "pipe",
				stderr: "pipe",
				env: {
					...process.env,
					DICTATE_SOCKET_PATH: daemon.socketPath,
				},
			});

			// Wait for connection
			await daemon.waitForMessage("start_listening", 2000);

			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});

			await new Promise((r) => setTimeout(r, 100));

			// Send SIGINT
			proc.kill("SIGINT");

			// Wait a bit for the stopping message
			await new Promise((r) => setTimeout(r, 100));

			// Send messages to trigger exit
			daemon.sendToAll({
				type: "status",
				state: "flushing",
				audio_ok: false,
				ws_ok: true,
			});
			daemon.sendToAll({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});

			const [_stdout, stderr] = await Promise.all([
				new Response(proc.stdout).text(),
				new Response(proc.stderr).text(),
			]);

			await proc.exited;

			// Should contain stopping message
			expect(stderr).toContain("[stopping]");
		});

		it("exits with code 0 after graceful shutdown", async () => {
			const cliPath = path.join(
				import.meta.dir,
				"..",
				"..",
				"cli",
				"dictate.ts",
			);

			const proc = Bun.spawn(["bun", cliPath, "--no-clipboard"], {
				stdout: "pipe",
				stderr: "pipe",
				env: {
					...process.env,
					DICTATE_SOCKET_PATH: daemon.socketPath,
				},
			});

			// Wait for connection
			await daemon.waitForMessage("start_listening", 2000);

			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});

			await new Promise((r) => setTimeout(r, 100));

			// Send SIGINT
			proc.kill("SIGINT");

			// Wait for stop_listening
			await daemon.waitForMessage("stop_listening", 1000);

			// Complete the shutdown sequence
			daemon.sendToAll({
				type: "status",
				state: "flushing",
				audio_ok: false,
				ws_ok: true,
			});
			daemon.sendToAll({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});

			await proc.exited;

			expect(proc.exitCode).toBe(0);
		});

		it("waits for final transcript before exiting", async () => {
			const cliPath = path.join(
				import.meta.dir,
				"..",
				"..",
				"cli",
				"dictate.ts",
			);

			const proc = Bun.spawn(
				["bun", cliPath, "--verbose", "--no-clipboard", "--stdout"],
				{
					stdout: "pipe",
					stderr: "pipe",
					env: {
						...process.env,
						DICTATE_SOCKET_PATH: daemon.socketPath,
					},
				},
			);

			// Wait for connection
			await daemon.waitForMessage("start_listening", 2000);

			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});

			await new Promise((r) => setTimeout(r, 100));

			// Send SIGINT
			proc.kill("SIGINT");

			// Wait for stop_listening
			await daemon.waitForMessage("stop_listening", 1000);

			// Send flushing state
			daemon.sendToAll({
				type: "status",
				state: "flushing",
				audio_ok: false,
				ws_ok: true,
			});

			// Delay before sending final transcript (simulating API latency)
			await new Promise((r) => setTimeout(r, 200));

			// Now send final transcript
			daemon.sendToAll({
				type: "final_transcript",
				item_id: "final_item",
				text: "final transcript text",
			});

			daemon.sendToAll({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});

			const [stdout, _stderr] = await Promise.all([
				new Response(proc.stdout).text(),
				new Response(proc.stderr).text(),
			]);

			await proc.exited;

			// Should have received the final transcript
			expect(stdout).toContain("final transcript text");
			expect(proc.exitCode).toBe(0);
		});
	});

	describe("Normal exit (daemon disconnect)", () => {
		it("exits cleanly when daemon disconnects after final transcript", async () => {
			const cliPath = path.join(
				import.meta.dir,
				"..",
				"..",
				"cli",
				"dictate.ts",
			);

			const proc = Bun.spawn(["bun", cliPath, "--no-clipboard"], {
				stdout: "pipe",
				stderr: "pipe",
				env: {
					...process.env,
					DICTATE_SOCKET_PATH: daemon.socketPath,
				},
			});

			// Wait for connection
			await daemon.waitForMessage("start_listening", 2000);

			// Simulate a full session
			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});

			await new Promise((r) => setTimeout(r, 50));

			daemon.sendToAll({
				type: "final_transcript",
				item_id: "item_1",
				text: "test",
			});

			// CLI only exits when daemon disconnects (after receiving final transcript)
			// or when SIGINT is sent. Here we simulate daemon disconnect.
			await new Promise((r) => setTimeout(r, 50));
			for (const client of daemon.clients) {
				client.end();
			}

			await proc.exited;

			// Exit code 0 because we received a final transcript before disconnect
			expect(proc.exitCode).toBe(0);
		});
	});
});

describe("CLI handles daemon disconnect", () => {
	let daemon: MockDaemon;

	beforeEach(async () => {
		daemon = await createMockDaemon();
	});

	afterEach(async () => {
		await daemon.cleanup();
	});

	it("exits when daemon closes connection", async () => {
		const cliPath = path.join(import.meta.dir, "..", "..", "cli", "dictate.ts");

		const proc = Bun.spawn(["bun", cliPath, "--verbose", "--no-clipboard"], {
			stdout: "pipe",
			stderr: "pipe",
			env: {
				...process.env,
				DICTATE_SOCKET_PATH: daemon.socketPath,
			},
		});

		// Wait for connection
		await daemon.waitForMessage("start_listening", 2000);

		// Close the socket
		for (const client of daemon.clients) {
			client.end();
		}

		const stderr = await new Response(proc.stderr).text();
		await proc.exited;

		// Should indicate disconnect
		expect(stderr).toContain("[disconnected]");
	});
});
