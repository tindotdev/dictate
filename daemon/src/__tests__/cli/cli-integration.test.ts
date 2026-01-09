/**
 * CLI Integration Tests
 *
 * Tests CLI behavior with a mock daemon socket server.
 * Verifies output formatting, message handling, and flag behavior.
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import * as fs from "node:fs";
import * as net from "node:net";
import * as os from "node:os";
import * as path from "node:path";
import type { ClientMessage, DaemonMessage } from "../../protocol.js";

// ============================================================================
// Mock Daemon Server
// ============================================================================

interface MockDaemon {
	socketPath: string;
	server: net.Server;
	clients: net.Socket[];
	receivedMessages: ClientMessage[];
	sendToAll: (msg: DaemonMessage) => void;
	sendSequence: (msgs: DaemonMessage[], delayMs?: number) => Promise<void>;
	waitForMessage: (type: string, timeoutMs?: number) => Promise<ClientMessage>;
	cleanup: () => Promise<void>;
}

async function createMockDaemon(): Promise<MockDaemon> {
	const testDir = fs.mkdtempSync(path.join(os.tmpdir(), "dictate-cli-test-"));
	const socketPath = path.join(testDir, "dictate.sock");

	const clients: net.Socket[] = [];
	const receivedMessages: ClientMessage[] = [];

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

	const sendSequence = async (msgs: DaemonMessage[], delayMs = 50) => {
		for (const msg of msgs) {
			sendToAll(msg);
			await new Promise((r) => setTimeout(r, delayMs));
		}
	};

	const waitForMessage = async (
		type: string,
		timeoutMs = 1000,
	): Promise<ClientMessage> => {
		const startTime = Date.now();
		while (Date.now() - startTime < timeoutMs) {
			const msg = receivedMessages.find((m) => m.type === type);
			if (msg) {
				return msg;
			}
			await new Promise((r) => setTimeout(r, 10));
		}
		throw new Error(
			`Timeout waiting for message type: ${type} after ${timeoutMs}ms`,
		);
	};

	const cleanup = async () => {
		for (const client of clients) {
			client.destroy();
		}
		// Wait for server to fully close
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
		sendSequence,
		waitForMessage,
		cleanup,
	};
}

// ============================================================================
// CLI Runner
// ============================================================================

interface CliResult {
	stdout: string;
	stderr: string;
	exitCode: number;
	stdoutLines: string[];
	stderrLines: string[];
}

async function runDictateCli(
	socketPath: string,
	args: string[] = [],
	options: { timeoutMs?: number; input?: string } = {},
): Promise<CliResult> {
	const cliPath = path.join(import.meta.dir, "..", "..", "cli", "dictate.ts");
	const { timeoutMs = 5000 } = options;

	const proc = Bun.spawn(["bun", cliPath, ...args], {
		stdout: "pipe",
		stderr: "pipe",
		env: {
			...process.env,
			DICTATE_SOCKET_PATH: socketPath,
		},
	});

	// Set up timeout
	const timeoutPromise = new Promise<never>((_, reject) => {
		setTimeout(() => {
			proc.kill();
			reject(new Error(`CLI timeout after ${timeoutMs}ms`));
		}, timeoutMs);
	});

	try {
		const [stdout, stderr] = await Promise.race([
			Promise.all([
				new Response(proc.stdout).text(),
				new Response(proc.stderr).text(),
			]),
			timeoutPromise,
		]);

		await proc.exited;

		return {
			stdout,
			stderr,
			exitCode: proc.exitCode ?? 1,
			stdoutLines: stdout.split("\n").filter(Boolean),
			stderrLines: stderr.split("\n").filter(Boolean),
		};
	} catch (error) {
		proc.kill();
		throw error;
	}
}

async function runDictatectlCli(
	socketPath: string,
	options: { timeoutMs?: number } = {},
): Promise<CliResult> {
	const cliPath = path.join(
		import.meta.dir,
		"..",
		"..",
		"cli",
		"dictatectl.ts",
	);
	const { timeoutMs = 5000 } = options;

	const proc = Bun.spawn(["bun", cliPath], {
		stdout: "pipe",
		stderr: "pipe",
		env: {
			...process.env,
			DICTATE_SOCKET_PATH: socketPath,
		},
	});

	const timeoutPromise = new Promise<never>((_, reject) => {
		setTimeout(() => {
			proc.kill();
			reject(new Error(`CLI timeout after ${timeoutMs}ms`));
		}, timeoutMs);
	});

	try {
		const [stdout, stderr] = await Promise.race([
			Promise.all([
				new Response(proc.stdout).text(),
				new Response(proc.stderr).text(),
			]),
			timeoutPromise,
		]);

		await proc.exited;

		return {
			stdout,
			stderr,
			exitCode: proc.exitCode ?? 1,
			stdoutLines: stdout.split("\n").filter(Boolean),
			stderrLines: stderr.split("\n").filter(Boolean),
		};
	} catch (error) {
		proc.kill();
		throw error;
	}
}

// ============================================================================
// Tests
// ============================================================================

describe("CLI Integration with Mock Daemon", () => {
	let daemon: MockDaemon;

	beforeEach(async () => {
		daemon = await createMockDaemon();
	});

	afterEach(async () => {
		// Kill any auto-started daemon processes that might interfere
		try {
			const pkill = Bun.spawn(["pkill", "-f", "main.ts"], {
				stdout: "ignore",
				stderr: "ignore",
			});
			await pkill.exited;
		} catch {
			// Ignore if pkill fails
		}
		await daemon.cleanup();
		// Allow time for spawned processes to fully terminate
		await new Promise((r) => setTimeout(r, 200));
	});

	describe("dictatectl JSONL output", () => {
		it("outputs JSONL to stdout", async () => {
			// Start CLI in background, then send messages
			const cliPromise = runDictatectlCli(daemon.socketPath, {
				timeoutMs: 2000,
			});

			// Wait for connection then send messages
			await new Promise((r) => setTimeout(r, 100));

			daemon.sendToAll({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
			});
			daemon.sendToAll({ type: "speech_started", item_id: "item_123" });
			daemon.sendToAll({
				type: "final_transcript",
				item_id: "item_123",
				text: "hello world",
			});

			// Close connections to end the CLI
			await new Promise((r) => setTimeout(r, 100));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			// Parse JSONL output
			const messages = result.stdoutLines.map((line) => JSON.parse(line));
			expect(messages.length).toBeGreaterThan(0);

			// Should have status messages
			const statusMsgs = messages.filter(
				(m: DaemonMessage) => m.type === "status",
			);
			expect(statusMsgs.length).toBeGreaterThan(0);
		});
	});

	describe("dictate CLI --json mode", () => {
		it("outputs JSONL with --json flag", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--json"], {
				timeoutMs: 2000,
			});

			await new Promise((r) => setTimeout(r, 100));

			daemon.sendSequence([
				{ type: "status", state: "listening", audio_ok: true, ws_ok: true },
				{
					type: "final_transcript",
					item_id: "item_1",
					text: "test transcript",
				},
				{ type: "status", state: "idle", audio_ok: false, ws_ok: false },
			]);

			await new Promise((r) => setTimeout(r, 200));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			// Should have JSON lines on stdout
			expect(result.stdoutLines.length).toBeGreaterThan(0);
			for (const line of result.stdoutLines) {
				expect(() => JSON.parse(line)).not.toThrow();
			}
		});
	});

	describe("dictate CLI --verbose mode", () => {
		it("shows debug output on stderr with --verbose", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--verbose"], {
				timeoutMs: 2000,
			});

			await new Promise((r) => setTimeout(r, 100));

			daemon.sendSequence([
				{ type: "status", state: "listening", audio_ok: true, ws_ok: true },
				{ type: "speech_started", item_id: "item_1" },
				{ type: "speech_stopped", item_id: "item_1" },
				{ type: "status", state: "idle", audio_ok: false, ws_ok: false },
			]);

			await new Promise((r) => setTimeout(r, 200));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			// Verbose output goes to stderr
			const stderrJoined = result.stderr;
			expect(stderrJoined).toContain("[connected]");
			expect(stderrJoined).toContain("[status]");
			expect(stderrJoined).toContain("[speech]");
		});

		it("shows status transitions with --verbose", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--verbose"], {
				timeoutMs: 2000,
			});

			// Wait for CLI to be ready (sent start_listening)
			await daemon.waitForMessage("start_listening");

			daemon.sendSequence([
				{
					type: "status",
					state: "audio_starting",
					audio_ok: false,
					ws_ok: false,
				},
				{ type: "status", state: "listening", audio_ok: true, ws_ok: true },
				{ type: "status", state: "flushing", audio_ok: false, ws_ok: true },
				{ type: "status", state: "idle", audio_ok: false, ws_ok: false },
			]);

			await new Promise((r) => setTimeout(r, 250));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			expect(result.stderr).toContain("audio_starting");
			expect(result.stderr).toContain("listening");
		});
	});

	describe("dictate CLI --stdout mode", () => {
		it("prints transcripts to stdout with --stdout", async () => {
			const cliPromise = runDictateCli(
				daemon.socketPath,
				["--stdout", "--no-clipboard"],
				{ timeoutMs: 2000 },
			);

			await new Promise((r) => setTimeout(r, 100));

			daemon.sendSequence([
				{ type: "status", state: "listening", audio_ok: true, ws_ok: true },
				{ type: "partial_transcript", item_id: "item_1", text: "hello" },
				{ type: "partial_transcript", item_id: "item_1", text: "hello world" },
				{
					type: "final_transcript",
					item_id: "item_1",
					text: "hello world test",
				},
				{ type: "status", state: "idle", audio_ok: false, ws_ok: false },
			]);

			await new Promise((r) => setTimeout(r, 200));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			// Final transcript should appear on stdout
			expect(result.stdout).toContain("hello world test");
		});
	});

	describe("dictate CLI sends correct messages", () => {
		it("sends start_listening on connect", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--no-clipboard"], {
				timeoutMs: 2000,
			});

			await new Promise((r) => setTimeout(r, 200));

			// Check received messages
			const startMsg = daemon.receivedMessages.find(
				(m) => m.type === "start_listening",
			);
			expect(startMsg).toBeDefined();

			// Clean up
			daemon.sendToAll({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});
			for (const client of daemon.clients) {
				client.end();
			}

			await cliPromise.catch(() => {}); // Ignore timeout
		});
	});

	describe("dictate CLI error handling", () => {
		it("exits with error on unrecoverable error message", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--no-clipboard"], {
				timeoutMs: 3000,
			});

			// Wait for CLI to be ready (sent start_listening)
			await daemon.waitForMessage("start_listening");

			daemon.sendToAll({
				type: "error",
				code: "INTERNAL_ERROR",
				message: "Something went wrong",
				recoverable: false,
			});

			const result = await cliPromise;

			expect(result.exitCode).toBe(1);
		});

		it("continues on recoverable error message", async () => {
			const cliPromise = runDictateCli(daemon.socketPath, ["--no-clipboard"], {
				timeoutMs: 2000,
			});

			await new Promise((r) => setTimeout(r, 100));

			// Send recoverable error followed by normal completion
			// CLI must receive a final_transcript to exit cleanly on disconnect
			daemon.sendSequence([
				{ type: "status", state: "listening", audio_ok: true, ws_ok: true },
				{
					type: "error",
					code: "NETWORK_ERROR",
					message: "Temporary issue",
					recoverable: true,
				},
				// Continue session after recoverable error
				{ type: "final_transcript", item_id: "item_1", text: "test" },
				{ type: "status", state: "idle", audio_ok: false, ws_ok: false },
			]);

			await new Promise((r) => setTimeout(r, 200));
			for (const client of daemon.clients) {
				client.end();
			}

			const result = await cliPromise;

			// Should not have exited with error (recoverable error was handled)
			expect(result.exitCode).toBe(0);
		});
	});
});

describe("CLI Connection Behavior", () => {
	// Note: When socket doesn't exist, CLI attempts auto-start which takes time.
	// The auto-start timeout behavior is tested in daemon-autostart tests.
	// Here we just verify the CLI handles connection errors gracefully.

	it("reports connection error in verbose mode", async () => {
		// Create a mock daemon but close it immediately to simulate connection failure
		const testDir = fs.mkdtempSync(
			path.join(os.tmpdir(), "dictate-cli-conn-test-"),
		);
		const socketPath = path.join(testDir, "dictate.sock");

		// Create a socket file (not a real server) to bypass auto-start check
		const server = net.createServer();
		await new Promise<void>((resolve) => {
			server.listen(socketPath, resolve);
		});
		// Close immediately so connection fails
		server.close();

		try {
			const result = await runDictateCli(socketPath, [
				"--verbose",
				"--no-clipboard",
			]).catch((e) => ({
				stderr: e.message,
				exitCode: 1,
				stdout: "",
				stdoutLines: [],
				stderrLines: [],
			}));

			// Should have some error indication
			expect(result.exitCode).not.toBe(0);
		} finally {
			fs.rmSync(testDir, { recursive: true, force: true });
		}
	});
});
