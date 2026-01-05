/**
 * Audio Backend Tests
 *
 * Tests audio backend command construction and validation across platforms.
 * These tests ensure backend configuration is correct and dependencies are available.
 */

import { describe, expect, it } from "bun:test";
import { createLinuxPwcatBackend } from "../../audio/backends/linux-pwcat.js";
import { createMacOSFfmpegBackend } from "../../audio/backends/macos-ffmpeg.js";
import { DEFAULT_SAMPLE_RATE } from "../../audio/index.js";

describe("Linux pw-cat backend", () => {
	it("has correct name", () => {
		const backend = createLinuxPwcatBackend();
		expect(backend.name).toBe("pw-cat (PipeWire)");
	});

	it("creates correct command with default options", () => {
		const backend = createLinuxPwcatBackend();
		const cmd = backend.getCommand();

		expect(cmd.command).toBe("pw-cat");
		expect(cmd.args).toContain("--record");
		expect(cmd.args).toContain("--raw");
		expect(cmd.args).toContain(`--rate=${DEFAULT_SAMPLE_RATE}`);
		expect(cmd.args).toContain("--channels=1");
		expect(cmd.args).toContain("--format=s16");
		expect(cmd.args).toContain("-");
	});

	it("uses custom sample rate when provided", () => {
		const backend = createLinuxPwcatBackend({ sampleRate: 16000 });
		const cmd = backend.getCommand();

		expect(cmd.args).toContain("--rate=16000");
	});

	it("uses custom device when provided", () => {
		const backend = createLinuxPwcatBackend({ device: "alsa_input.usb" });
		const cmd = backend.getCommand();

		expect(cmd.args).toContain("--target=alsa_input.usb");
	});

	it.skipIf(process.platform !== "linux")(
		"validates pw-cat availability on Linux",
		async () => {
			const backend = createLinuxPwcatBackend();
			const error = await backend.validate();

			// On Linux: null if pw-cat available, error message if not
			if (error !== null) {
				expect(error).toContain("pw-cat not found");
				expect(error).toContain("pipewire");
			}
		},
	);
});

describe("macOS ffmpeg backend", () => {
	it("has correct name", () => {
		const backend = createMacOSFfmpegBackend();
		expect(backend.name).toBe("ffmpeg (avfoundation)");
	});

	it("creates correct command with default options", () => {
		const backend = createMacOSFfmpegBackend();
		const cmd = backend.getCommand();

		expect(cmd.command).toBe("ffmpeg");
		expect(cmd.args).toContain("-f");
		expect(cmd.args).toContain("avfoundation");
		expect(cmd.args).toContain("-i");
		expect(cmd.args).toContain(":default");
		expect(cmd.args).toContain("-ar");
		expect(cmd.args).toContain(DEFAULT_SAMPLE_RATE.toString());
		expect(cmd.args).toContain("-ac");
		expect(cmd.args).toContain("1");
		expect(cmd.args).toContain("-f");
		expect(cmd.args).toContain("s16le");
		expect(cmd.args).toContain("-");
	});

	it("uses custom sample rate when provided", () => {
		const backend = createMacOSFfmpegBackend({ sampleRate: 16000 });
		const cmd = backend.getCommand();

		expect(cmd.args).toContain("-ar");
		expect(cmd.args).toContain("16000");
	});

	it("uses custom device when provided", () => {
		const backend = createMacOSFfmpegBackend({ device: ":0" });
		const cmd = backend.getCommand();

		expect(cmd.args).toContain("-i");
		expect(cmd.args).toContain(":0");
	});

	it("defaults to system default microphone", () => {
		const backend = createMacOSFfmpegBackend();
		const cmd = backend.getCommand();

		// Should use ":default" for default audio input
		expect(cmd.args).toContain(":default");
	});

	it.skipIf(process.platform !== "darwin")(
		"validates ffmpeg availability on macOS",
		async () => {
			const backend = createMacOSFfmpegBackend();
			const error = await backend.validate();

			// On macOS: null if ffmpeg available, error message if not
			if (error !== null) {
				expect(error).toContain("ffmpeg");
				expect(error).toContain("brew install ffmpeg");
			}
		},
	);

	it.skipIf(process.platform !== "darwin")(
		"validates avfoundation support on macOS",
		async () => {
			const backend = createMacOSFfmpegBackend();
			const error = await backend.validate();

			// If ffmpeg is installed but lacks avfoundation, should error
			if (error !== null && !error.includes("not found")) {
				expect(error).toContain("avfoundation");
				expect(error).toContain("brew reinstall ffmpeg");
			}
		},
	);
});

describe("Audio backend factory", () => {
	it("exports DEFAULT_SAMPLE_RATE constant", () => {
		expect(DEFAULT_SAMPLE_RATE).toBe(24000);
	});

	it("backend commands are properly formatted", () => {
		// Test that both backends return valid command structures
		const linuxBackend = createLinuxPwcatBackend();
		const macBackend = createMacOSFfmpegBackend();

		const linuxCmd = linuxBackend.getCommand();
		const macCmd = macBackend.getCommand();

		// Both should have command and args
		expect(linuxCmd.command).toBeTruthy();
		expect(Array.isArray(linuxCmd.args)).toBe(true);
		expect(macCmd.command).toBeTruthy();
		expect(Array.isArray(macCmd.args)).toBe(true);
	});
});
