import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import {
	ConfigSchema,
	loadConfig,
	TranscriptionModelSchema,
} from "../config.js";

describe("ConfigSchema", () => {
	const originalEnv = process.env;

	beforeEach(() => {
		process.env = { ...originalEnv };
	});

	afterEach(() => {
		process.env = originalEnv;
	});

	it("validates complete config", () => {
		const result = ConfigSchema.safeParse({
			apiKey: "sk-test-key",
			model: "gpt-4o-mini-transcribe",
			prompt: "technical terms",
			vadThreshold: 0.6,
			vadPrefixPaddingMs: 400,
			vadSilenceDurationMs: 600,
			noiseReduction: "near_field",
			includeLogprobs: false,
			debug: true,
		});
		expect(result.success).toBe(true);
	});

	it("applies defaults for optional fields", () => {
		const result = ConfigSchema.safeParse({
			apiKey: "sk-test-key",
		});
		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data.model).toBe("gpt-4o-transcribe");
			expect(result.data.vadThreshold).toBe(0.5);
			expect(result.data.vadPrefixPaddingMs).toBe(300);
			expect(result.data.vadSilenceDurationMs).toBe(500);
			expect(result.data.noiseReduction).toBe("near_field");
			expect(result.data.includeLogprobs).toBe(false);
			expect(result.data.debug).toBe(false);
		}
	});

	it("requires apiKey", () => {
		const result = ConfigSchema.safeParse({});
		expect(result.success).toBe(false);
		if (!result.success) {
			const apiKeyError = result.error.issues.find(
				(issue) => issue.path[0] === "apiKey",
			);
			expect(apiKeyError).toBeDefined();
		}
	});

	it("rejects empty apiKey", () => {
		const result = ConfigSchema.safeParse({ apiKey: "" });
		expect(result.success).toBe(false);
	});

	it("validates vadThreshold range", () => {
		// Valid: 0 to 1
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadThreshold: 0 }).success,
		).toBe(true);
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadThreshold: 0.5 }).success,
		).toBe(true);
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadThreshold: 1 }).success,
		).toBe(true);

		// Invalid: outside range
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadThreshold: -0.1 }).success,
		).toBe(false);
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadThreshold: 1.1 }).success,
		).toBe(false);
	});

	it("validates positive values for timing fields", () => {
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadPrefixPaddingMs: 0 }).success,
		).toBe(false);
		expect(
			ConfigSchema.safeParse({ apiKey: "key", vadSilenceDurationMs: -100 })
				.success,
		).toBe(false);
	});

	it("allows null noiseReduction", () => {
		const result = ConfigSchema.safeParse({
			apiKey: "sk-test",
			noiseReduction: null,
		});
		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data.noiseReduction).toBeNull();
		}
	});
});

describe("TranscriptionModelSchema", () => {
	it("accepts valid models", () => {
		expect(
			TranscriptionModelSchema.safeParse("gpt-4o-transcribe").success,
		).toBe(true);
		expect(
			TranscriptionModelSchema.safeParse("gpt-4o-mini-transcribe").success,
		).toBe(true);
	});

	it("rejects invalid models", () => {
		expect(TranscriptionModelSchema.safeParse("whisper-1").success).toBe(false);
		expect(TranscriptionModelSchema.safeParse("gpt-4").success).toBe(false);
	});
});

describe("loadConfig", () => {
	const originalEnv = process.env;

	beforeEach(() => {
		process.env = { ...originalEnv };
	});

	afterEach(() => {
		process.env = originalEnv;
	});

	it("loads config with valid minimal environment", () => {
		process.env.OPENAI_API_KEY = "sk-test-key-123";
		const config = loadConfig();

		expect(config.apiKey).toBe("sk-test-key-123");
		expect(config.model).toBe("gpt-4o-transcribe");
		expect(config.language).toBe("en");
		expect(config.debug).toBe(false);
	});

	it("loads config with all environment variables set", () => {
		process.env.OPENAI_API_KEY = "sk-prod-key";
		process.env.OPENAI_STT_MODEL = "gpt-4o-mini-transcribe";
		process.env.OPENAI_STT_LANGUAGE = "es";
		process.env.OPENAI_STT_PROMPT = "Custom prompt for Spanish";
		process.env.DEBUG = "1";

		const config = loadConfig();

		expect(config.apiKey).toBe("sk-prod-key");
		expect(config.model).toBe("gpt-4o-mini-transcribe");
		expect(config.language).toBe("es");
		expect(config.prompt).toBe("Custom prompt for Spanish");
		expect(config.debug).toBe(true);
	});

	it("uses default prompt when OPENAI_STT_PROMPT not set", () => {
		process.env.OPENAI_API_KEY = "sk-test";
		const config = loadConfig();

		expect(config.prompt).toContain("TypeScript");
		expect(config.prompt).toContain("JavaScript");
	});

	it("sets debug to false when DEBUG is not '1'", () => {
		process.env.OPENAI_API_KEY = "sk-test";
		process.env.DEBUG = "0";
		const config1 = loadConfig();
		expect(config1.debug).toBe(false);

		process.env.DEBUG = "true";
		const config2 = loadConfig();
		expect(config2.debug).toBe(false);

		delete process.env.DEBUG;
		const config3 = loadConfig();
		expect(config3.debug).toBe(false);
	});

	it("sets debug to true when DEBUG is '1'", () => {
		process.env.OPENAI_API_KEY = "sk-test";
		process.env.DEBUG = "1";
		const config = loadConfig();
		expect(config.debug).toBe(true);
	});

	it("throws error when OPENAI_API_KEY is missing", () => {
		delete process.env.OPENAI_API_KEY;
		expect(() => loadConfig()).toThrow("Configuration error");
		expect(() => loadConfig()).toThrow("apiKey");
	});

	it("throws error when OPENAI_API_KEY is empty string", () => {
		process.env.OPENAI_API_KEY = "";
		expect(() => loadConfig()).toThrow("Configuration error");
		expect(() => loadConfig()).toThrow("apiKey");
	});

	it("throws error for invalid model", () => {
		process.env.OPENAI_API_KEY = "sk-test";
		process.env.OPENAI_STT_MODEL = "whisper-1";
		expect(() => loadConfig()).toThrow("Configuration error");
		expect(() => loadConfig()).toThrow("model");
	});

	it("throws error for invalid language code (not 2 chars)", () => {
		process.env.OPENAI_API_KEY = "sk-test";
		process.env.OPENAI_STT_LANGUAGE = "eng";
		expect(() => loadConfig()).toThrow("Configuration error");
		expect(() => loadConfig()).toThrow("language");
	});

	it("throws error with multiple validation errors aggregated", () => {
		process.env.OPENAI_API_KEY = "";
		process.env.OPENAI_STT_MODEL = "invalid-model";
		process.env.OPENAI_STT_LANGUAGE = "invalid";

		try {
			loadConfig();
			expect.unreachable("Should have thrown");
		} catch (err) {
			const message = (err as Error).message;
			expect(message).toContain("Configuration error");
			expect(message).toContain("apiKey");
			expect(message).toContain("model");
			expect(message).toContain("language");
		}
	});
});
