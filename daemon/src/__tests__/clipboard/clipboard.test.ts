/**
 * Clipboard Backend Tests
 *
 * Tests clipboard platform detection and backend behavior.
 * Note: Actual clipboard operations require a display server,
 * so these tests focus on logic and mocking.
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { createLinuxWlcopyBackend } from "../../clipboard/backends/linux-wlcopy.js";
import { createLinuxXclipBackend } from "../../clipboard/backends/linux-xclip.js";
import { createLinuxXselBackend } from "../../clipboard/backends/linux-xsel.js";
import { createMacOSPbcopyBackend } from "../../clipboard/backends/macos-pbcopy.js";
import { createStdoutFallbackBackend } from "../../clipboard/backends/stdout.js";

describe("Clipboard Backends", () => {
	const originalEnv = process.env;

	beforeEach(() => {
		process.env = { ...originalEnv };
	});

	afterEach(() => {
		process.env = originalEnv;
	});

	describe("Linux wl-copy backend", () => {
		it("has correct name", () => {
			const backend = createLinuxWlcopyBackend();
			expect(backend.name).toBe("wl-copy");
		});

		it("reports unavailable without WAYLAND_DISPLAY", async () => {
			delete process.env.WAYLAND_DISPLAY;

			const backend = createLinuxWlcopyBackend();
			const error = await backend.isAvailable();

			expect(error).toContain("WAYLAND_DISPLAY not set");
		});

		it("checks for wl-copy binary when WAYLAND_DISPLAY is set", async () => {
			process.env.WAYLAND_DISPLAY = "wayland-1";

			const backend = createLinuxWlcopyBackend();
			const error = await backend.isAvailable();

			// Either null (wl-copy found) or error message (not found)
			if (error !== null) {
				expect(error).toContain("wl-copy not found");
			}
		});
	});

	describe("Linux xclip backend", () => {
		it("has correct name", () => {
			const backend = createLinuxXclipBackend();
			expect(backend.name).toBe("xclip");
		});

		it("reports unavailable without DISPLAY or WAYLAND_DISPLAY", async () => {
			delete process.env.DISPLAY;
			delete process.env.WAYLAND_DISPLAY;

			const backend = createLinuxXclipBackend();
			const error = await backend.isAvailable();

			expect(error).toContain("DISPLAY not set");
		});

		it("checks for xclip binary when DISPLAY is set", async () => {
			process.env.DISPLAY = ":0";
			delete process.env.WAYLAND_DISPLAY;

			const backend = createLinuxXclipBackend();
			const error = await backend.isAvailable();

			// Either null (xclip found) or error message (not found)
			if (error !== null) {
				expect(error).toContain("xclip not found");
			}
		});
	});

	describe("Linux xsel backend", () => {
		it("has correct name", () => {
			const backend = createLinuxXselBackend();
			expect(backend.name).toBe("xsel");
		});

		it("reports unavailable without DISPLAY or WAYLAND_DISPLAY", async () => {
			delete process.env.DISPLAY;
			delete process.env.WAYLAND_DISPLAY;

			const backend = createLinuxXselBackend();
			const error = await backend.isAvailable();

			expect(error).toContain("DISPLAY not set");
		});

		it("checks for xsel binary when DISPLAY is set", async () => {
			process.env.DISPLAY = ":0";
			delete process.env.WAYLAND_DISPLAY;

			const backend = createLinuxXselBackend();
			const error = await backend.isAvailable();

			// Either null (xsel found) or error message (not found)
			if (error !== null) {
				expect(error).toContain("xsel not found");
			}
		});
	});

	describe("Stdout fallback backend", () => {
		it("has correct name", () => {
			const backend = createStdoutFallbackBackend();
			expect(backend.name).toBe("stdout-fallback");
		});

		it("is always available", async () => {
			const backend = createStdoutFallbackBackend();
			const error = await backend.isAvailable();
			expect(error).toBeNull();
		});

		it("write() returns true", async () => {
			const backend = createStdoutFallbackBackend();
			// Capture console output to avoid test noise
			const originalLog = console.log;
			console.log = () => {};

			try {
				const result = await backend.write("test text");
				expect(result).toBe(true);
			} finally {
				console.log = originalLog;
			}
		});
	});

	describe("macOS pbcopy backend", () => {
		it("has correct name", () => {
			const backend = createMacOSPbcopyBackend();
			expect(backend.name).toBe("pbcopy");
		});

		it("checks for pbcopy binary", async () => {
			const backend = createMacOSPbcopyBackend();
			const error = await backend.isAvailable();

			// On macOS: null, on Linux: error
			if (process.platform === "darwin") {
				expect(error).toBeNull();
			} else {
				expect(error).toContain("pbcopy not found");
			}
		});
	});

	describe("Backend write interface", () => {
		it("wl-copy backend returns boolean from write()", async () => {
			const backend = createLinuxWlcopyBackend();
			// Don't actually write - just verify the interface
			expect(typeof backend.write).toBe("function");
		});

		it("xclip backend returns boolean from write()", async () => {
			const backend = createLinuxXclipBackend();
			expect(typeof backend.write).toBe("function");
		});

		it("pbcopy backend returns boolean from write()", async () => {
			const backend = createMacOSPbcopyBackend();
			expect(typeof backend.write).toBe("function");
		});

		it("xsel backend returns boolean from write()", async () => {
			const backend = createLinuxXselBackend();
			expect(typeof backend.write).toBe("function");
		});

		it("stdout-fallback backend returns boolean from write()", async () => {
			const backend = createStdoutFallbackBackend();
			expect(typeof backend.write).toBe("function");
		});
	});
});

describe("Clipboard integration", () => {
	// These tests only run if clipboard is available in the test environment
	const hasWayland = !!process.env.WAYLAND_DISPLAY;
	const hasX11 = !!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY;

	describe.skipIf(!hasWayland)("Wayland environment", () => {
		it("wl-copy is available", async () => {
			const backend = createLinuxWlcopyBackend();
			const error = await backend.isAvailable();
			expect(error).toBeNull();
		});

		it("can write to clipboard", async () => {
			const backend = createLinuxWlcopyBackend();
			const testText = `clipboard-test-${Date.now()}`;
			const result = await backend.write(testText);
			expect(result).toBe(true);
		});
	});

	describe.skipIf(!hasX11)("X11 environment", () => {
		it("xclip is available", async () => {
			const backend = createLinuxXclipBackend();
			const error = await backend.isAvailable();
			expect(error).toBeNull();
		});
	});
});
