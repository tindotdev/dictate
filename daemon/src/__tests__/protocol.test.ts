import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import {
	ClientMessageSchema,
	DaemonMessageSchema,
	DaemonStateSchema,
	DictatectlMessageSchema,
	ErrorCodeSchema,
	emit,
	emitDebug,
	emitError,
	emitFinalTranscript,
	emitInitialized,
	emitPartialTranscript,
	emitSpeechStarted,
	emitSpeechStopped,
	emitStatus,
} from "../protocol.js";

describe("ClientMessageSchema", () => {
	describe("new protocol messages", () => {
		it("validates initialize command", () => {
			const result = ClientMessageSchema.safeParse({
				type: "initialize",
				version: "2.0.0",
			});
			expect(result.success).toBe(true);
		});

		it("validates initialize with client_id", () => {
			const result = ClientMessageSchema.safeParse({
				type: "initialize",
				client_id: "nvim-123",
				version: "2.0.0",
			});
			expect(result.success).toBe(true);
		});

		it("validates start_listening command", () => {
			const result = ClientMessageSchema.safeParse({ type: "start_listening" });
			expect(result.success).toBe(true);
		});

		it("validates stop_listening command", () => {
			const result = ClientMessageSchema.safeParse({ type: "stop_listening" });
			expect(result.success).toBe(true);
		});

		it("validates set_mode command", () => {
			const result = ClientMessageSchema.safeParse({
				type: "set_mode",
				mode: "dictation",
			});
			expect(result.success).toBe(true);
		});

		it("validates disconnect command", () => {
			const result = ClientMessageSchema.safeParse({ type: "disconnect" });
			expect(result.success).toBe(true);
		});
	});

	it("rejects invalid command type", () => {
		const result = ClientMessageSchema.safeParse({ type: "invalid" });
		expect(result.success).toBe(false);
	});

	it("rejects missing type", () => {
		const result = ClientMessageSchema.safeParse({});
		expect(result.success).toBe(false);
	});
});

describe("DaemonMessageSchema", () => {
	describe("initialized message", () => {
		it("validates initialized response", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "initialized",
				client_id: "client-123",
				daemon_version: "0.2.0",
			});
			expect(result.success).toBe(true);
		});
	});

	describe("status message", () => {
		it("validates status message with all fields", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "status",
				state: "listening",
				audio_ok: true,
				ws_ok: true,
				message: "Connected",
			});
			expect(result.success).toBe(true);
		});

		it("validates status message without optional message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "status",
				state: "idle",
				audio_ok: false,
				ws_ok: false,
			});
			expect(result.success).toBe(true);
		});

		it("validates all daemon states", () => {
			const states = [
				"idle",
				"audio_starting",
				"listening",
				"flushing",
				"reconnecting",
				"error",
			];
			for (const state of states) {
				const result = DaemonMessageSchema.safeParse({
					type: "status",
					state,
					audio_ok: true,
					ws_ok: true,
				});
				expect(result.success).toBe(true);
			}
		});

		it("rejects invalid status state", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "status",
				state: "invalid_state",
				audio_ok: true,
				ws_ok: true,
			});
			expect(result.success).toBe(false);
		});

		it("rejects status without audio_ok", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "status",
				state: "idle",
				ws_ok: true,
			});
			expect(result.success).toBe(false);
		});
	});

	describe("transcription messages", () => {
		it("validates speech_started message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "speech_started",
				item_id: "item_456",
			});
			expect(result.success).toBe(true);
		});

		it("validates speech_stopped message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "speech_stopped",
				item_id: "item_456",
			});
			expect(result.success).toBe(true);
		});

		it("validates partial_transcript message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "partial_transcript",
				item_id: "item_123",
				text: "hello world",
			});
			expect(result.success).toBe(true);
		});

		it("validates final_transcript message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "final_transcript",
				item_id: "item_123",
				text: "Hello, world!",
			});
			expect(result.success).toBe(true);
		});
	});

	describe("error message", () => {
		it("validates error message with all fields", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "error",
				code: "AUDIO_UNAVAILABLE",
				message: "Failed to start audio capture",
				recoverable: true,
				hint: "Check if pw-cat is installed",
			});
			expect(result.success).toBe(true);
		});

		it("validates error message without hint", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "error",
				code: "AUTH_FAILED",
				message: "Invalid API key",
				recoverable: false,
			});
			expect(result.success).toBe(true);
		});

		it("validates all error codes", () => {
			const codes = [
				"DAEMON_UNAVAILABLE",
				"AUTH_FAILED",
				"CONFIG_ERROR",
				"AUDIO_UNAVAILABLE",
				"NETWORK_ERROR",
				"RATE_LIMITED",
				"INTERNAL_ERROR",
			];
			for (const code of codes) {
				const result = DaemonMessageSchema.safeParse({
					type: "error",
					code,
					message: "test",
					recoverable: true,
				});
				expect(result.success).toBe(true);
			}
		});

		it("rejects error without recoverable flag", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "error",
				code: "AUTH_FAILED",
				message: "Invalid API key",
			});
			expect(result.success).toBe(false);
		});
	});

	describe("debug message", () => {
		it("validates debug message", () => {
			const result = DaemonMessageSchema.safeParse({
				type: "debug",
				message: "WebSocket connected",
			});
			expect(result.success).toBe(true);
		});
	});

	it("rejects invalid message type", () => {
		const result = DaemonMessageSchema.safeParse({
			type: "unknown",
			data: "test",
		});
		expect(result.success).toBe(false);
	});
});

describe("DictatectlMessageSchema", () => {
	it("validates dictatectl status message", () => {
		const result = DictatectlMessageSchema.safeParse({
			type: "status",
			state: "connecting",
		});
		expect(result.success).toBe(true);
	});

	it("validates all dictatectl states", () => {
		const states = ["connecting", "connected", "reconnecting"];
		for (const state of states) {
			const result = DictatectlMessageSchema.safeParse({
				type: "status",
				state,
			});
			expect(result.success).toBe(true);
		}
	});

	it("validates dictatectl DAEMON_UNAVAILABLE error", () => {
		const result = DictatectlMessageSchema.safeParse({
			type: "error",
			code: "DAEMON_UNAVAILABLE",
			message: "Cannot connect to dictate daemon",
			recoverable: false,
			hint: "Run: systemctl --user enable --now dictate.service",
		});
		expect(result.success).toBe(true);
	});
});

describe("DaemonStateSchema", () => {
	it("accepts valid states", () => {
		const states = [
			"idle",
			"audio_starting",
			"listening",
			"flushing",
			"reconnecting",
			"error",
		];
		for (const state of states) {
			const result = DaemonStateSchema.safeParse(state);
			expect(result.success).toBe(true);
		}
	});

	it("rejects invalid state", () => {
		const result = DaemonStateSchema.safeParse("invalid");
		expect(result.success).toBe(false);
	});
});

describe("ErrorCodeSchema", () => {
	it("accepts valid error codes", () => {
		const codes = [
			"DAEMON_UNAVAILABLE",
			"AUTH_FAILED",
			"CONFIG_ERROR",
			"AUDIO_UNAVAILABLE",
			"NETWORK_ERROR",
			"RATE_LIMITED",
			"INTERNAL_ERROR",
		];
		for (const code of codes) {
			const result = ErrorCodeSchema.safeParse(code);
			expect(result.success).toBe(true);
		}
	});

	it("rejects invalid error code", () => {
		const result = ErrorCodeSchema.safeParse("INVALID_CODE");
		expect(result.success).toBe(false);
	});
});

describe("Emit Functions", () => {
	let stdoutWrites: string[];
	const originalWrite = process.stdout.write;
	const originalEnv = process.env;

	beforeEach(() => {
		stdoutWrites = [];
		// biome-ignore lint: test mock
		process.stdout.write = ((chunk: string) => {
			stdoutWrites.push(chunk);
			return true;
		}) as any;
		process.env = { ...originalEnv };
	});

	afterEach(() => {
		process.stdout.write = originalWrite;
		process.env = originalEnv;
	});

	describe("emit", () => {
		it("writes JSON message to stdout with newline", () => {
			emit({ type: "status", state: "idle", audio_ok: true, ws_ok: true });

			expect(stdoutWrites.length).toBe(1);
			expect(stdoutWrites[0]).toContain('"type":"status"');
			expect(stdoutWrites[0]).toContain('"state":"idle"');
			expect(stdoutWrites[0]).toEndWith("\n");
		});

		it("serializes message correctly", () => {
			emit({
				type: "partial_transcript",
				item_id: "item_123",
				text: "hello world",
			});

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("partial_transcript");
			expect(parsed.item_id).toBe("item_123");
			expect(parsed.text).toBe("hello world");
		});
	});

	describe("emitStatus", () => {
		it("emits status message with all parameters", () => {
			emitStatus("listening", true, true, "Connected");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("status");
			expect(parsed.state).toBe("listening");
			expect(parsed.audio_ok).toBe(true);
			expect(parsed.ws_ok).toBe(true);
			expect(parsed.message).toBe("Connected");
		});

		it("emits status message without optional message", () => {
			emitStatus("idle", false, false);

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("status");
			expect(parsed.state).toBe("idle");
			expect(parsed.audio_ok).toBe(false);
			expect(parsed.ws_ok).toBe(false);
			expect(parsed.message).toBeUndefined();
		});

		it("emits all daemon states", () => {
			const states = [
				"idle",
				"audio_starting",
				"listening",
				"flushing",
				"reconnecting",
				"error",
			] as const;

			for (const state of states) {
				emitStatus(state, true, true);
			}

			expect(stdoutWrites.length).toBe(states.length);
		});
	});

	describe("emitError", () => {
		it("emits error with all parameters", () => {
			emitError(
				"AUDIO_UNAVAILABLE",
				"Failed to start audio",
				true,
				"Check if pw-cat is installed",
			);

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("error");
			expect(parsed.code).toBe("AUDIO_UNAVAILABLE");
			expect(parsed.message).toBe("Failed to start audio");
			expect(parsed.recoverable).toBe(true);
			expect(parsed.hint).toBe("Check if pw-cat is installed");
		});

		it("emits error without optional hint", () => {
			emitError("AUTH_FAILED", "Invalid API key", false);

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("error");
			expect(parsed.code).toBe("AUTH_FAILED");
			expect(parsed.message).toBe("Invalid API key");
			expect(parsed.recoverable).toBe(false);
			expect(parsed.hint).toBeUndefined();
		});

		it("emits all error codes", () => {
			const codes = [
				"DAEMON_UNAVAILABLE",
				"AUTH_FAILED",
				"CONFIG_ERROR",
				"AUDIO_UNAVAILABLE",
				"NETWORK_ERROR",
				"RATE_LIMITED",
				"INTERNAL_ERROR",
			] as const;

			for (const code of codes) {
				emitError(code, `Test ${code}`, true);
			}

			expect(stdoutWrites.length).toBe(codes.length);
		});
	});

	describe("emitDebug", () => {
		it("emits debug message when DEBUG=1", () => {
			process.env.DEBUG = "1";
			emitDebug("WebSocket connected");

			expect(stdoutWrites.length).toBe(1);
			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("debug");
			expect(parsed.message).toBe("WebSocket connected");
		});

		it("does not emit debug message when DEBUG not set", () => {
			delete process.env.DEBUG;
			emitDebug("This should not be emitted");

			expect(stdoutWrites.length).toBe(0);
		});

		it("does not emit debug message when DEBUG=0", () => {
			process.env.DEBUG = "0";
			emitDebug("This should not be emitted");

			expect(stdoutWrites.length).toBe(0);
		});

		it("does not emit debug message when DEBUG is not '1'", () => {
			process.env.DEBUG = "true";
			emitDebug("This should not be emitted");

			expect(stdoutWrites.length).toBe(0);
		});
	});

	describe("emitPartialTranscript", () => {
		it("emits partial transcript message", () => {
			emitPartialTranscript("item_abc123", "hello world");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("partial_transcript");
			expect(parsed.item_id).toBe("item_abc123");
			expect(parsed.text).toBe("hello world");
		});
	});

	describe("emitFinalTranscript", () => {
		it("emits final transcript message", () => {
			emitFinalTranscript("item_xyz789", "Hello, world!");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("final_transcript");
			expect(parsed.item_id).toBe("item_xyz789");
			expect(parsed.text).toBe("Hello, world!");
		});
	});

	describe("emitSpeechStarted", () => {
		it("emits speech_started message", () => {
			emitSpeechStarted("item_start_1");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("speech_started");
			expect(parsed.item_id).toBe("item_start_1");
		});
	});

	describe("emitSpeechStopped", () => {
		it("emits speech_stopped message", () => {
			emitSpeechStopped("item_stop_1");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("speech_stopped");
			expect(parsed.item_id).toBe("item_stop_1");
		});
	});

	describe("emitInitialized", () => {
		it("emits initialized message with client_id and version", () => {
			emitInitialized("client_nvim_123");

			const written = stdoutWrites[0];
			const parsed = JSON.parse(written.trim());
			expect(parsed.type).toBe("initialized");
			expect(parsed.client_id).toBe("client_nvim_123");
			expect(parsed.daemon_version).toBeDefined();
		});
	});
});
