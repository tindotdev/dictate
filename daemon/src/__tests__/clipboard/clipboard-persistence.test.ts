/**
 * Clipboard Persistence Tests
 *
 * Tests that clipboard content persists after the write operation completes.
 * This catches bugs where the clipboard tool's background process gets killed
 * when the parent process exits.
 *
 * These tests require a running display server (Wayland or X11).
 */

import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { createLinuxWlcopyBackend } from "../../clipboard/backends/linux-wlcopy.js";
import { createLinuxXselBackend } from "../../clipboard/backends/linux-xsel.js";
import { createMacOSPbcopyBackend } from "../../clipboard/backends/macos-pbcopy.js";

const hasWayland = !!process.env.WAYLAND_DISPLAY;
const isMacOS = process.platform === "darwin";

describe.skipIf(!hasWayland)("Clipboard Persistence (Wayland)", () => {
	let originalClipboard: string | null = null;

	beforeEach(async () => {
		// Save original clipboard content to restore later
		try {
			const proc = Bun.spawn(["wl-paste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			await proc.exited;
			if (proc.exitCode === 0) {
				originalClipboard = await new Response(proc.stdout).text();
			}
		} catch {
			originalClipboard = null;
		}
	});

	afterEach(async () => {
		// Restore original clipboard if we saved it
		if (originalClipboard !== null) {
			const proc = Bun.spawn(["wl-copy", "--", originalClipboard], {
				stdin: "ignore",
				stdout: "ignore",
				stderr: "ignore",
			});
			await proc.exited;
		}
	});

	it("wl-copy is called with text as argument (not stdin)", async () => {
		const backend = createLinuxWlcopyBackend();
		const testText = `persistence-test-${Date.now()}`;

		const result = await backend.write(testText);
		expect(result).toBe(true);

		// Verify wl-copy process is running with our text as argument
		const pgrep = Bun.spawn(["pgrep", "-a", "wl-copy"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const output = await new Response(pgrep.stdout).text();
		await pgrep.exited;

		// Should find a wl-copy process with our text
		expect(output).toContain(testText);
	});

	it("clipboard content persists after write() returns", async () => {
		const backend = createLinuxWlcopyBackend();
		const testText = `persistence-test-${Date.now()}`;

		// Write to clipboard
		const writeResult = await backend.write(testText);
		expect(writeResult).toBe(true);

		// Small delay to ensure wl-copy has forked
		await new Promise((r) => setTimeout(r, 100));

		// Read back with wl-paste
		const pasteProc = Bun.spawn(["wl-paste"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const pasteOutput = await new Response(pasteProc.stdout).text();
		await pasteProc.exited;

		expect(pasteProc.exitCode).toBe(0);
		expect(pasteOutput.trim()).toBe(testText);
	});

	it("clipboard survives multiple sequential writes", async () => {
		const backend = createLinuxWlcopyBackend();
		const texts = [
			`test-1-${Date.now()}`,
			`test-2-${Date.now()}`,
			`test-3-${Date.now()}`,
		];

		for (const text of texts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			// Verify each write
			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["wl-paste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			expect(pasteOutput.trim()).toBe(text);
		}
	});

	it("handles text with special characters", async () => {
		const backend = createLinuxWlcopyBackend();
		const specialTexts = [
			"Hello, world!",
			"Line1\nLine2\nLine3",
			"Tab\there",
			"Quotes: \"double\" and 'single'",
			"Unicode: 日本語 emoji 🎉",
			"Dollars: $100 and backticks: `code`",
		];

		for (const text of specialTexts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["wl-paste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			// wl-paste may add a trailing newline, so trim both
			expect(pasteOutput.trim()).toBe(text.trim());
		}
	});

	it("handles empty string gracefully", async () => {
		const backend = createLinuxWlcopyBackend();

		// Empty string should still work (clears clipboard)
		const result = await backend.write("");
		expect(result).toBe(true);
	});

	it("spawns wl-copy with correct arguments", async () => {
		// Kill any existing wl-copy processes for clean test
		const killProc = Bun.spawn(["pkill", "-f", "wl-copy.*test-args"], {
			stdout: "ignore",
			stderr: "ignore",
		});
		await killProc.exited;

		const backend = createLinuxWlcopyBackend();
		const testText = "test-args-verification";

		await backend.write(testText);

		// Check that wl-copy was called with -- separator
		const pgrep = Bun.spawn(["pgrep", "-a", "wl-copy"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const output = await new Response(pgrep.stdout).text();
		await pgrep.exited;

		// Should contain "wl-copy -- <text>"
		expect(output).toContain("wl-copy");
		expect(output).toContain(testText);
	});
});

describe.skipIf(!isMacOS)("Clipboard Persistence (macOS)", () => {
	let originalClipboard: string | null = null;

	beforeEach(async () => {
		// Save original clipboard content to restore later
		try {
			const proc = Bun.spawn(["pbpaste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			await proc.exited;
			if (proc.exitCode === 0) {
				originalClipboard = await new Response(proc.stdout).text();
			}
		} catch {
			originalClipboard = null;
		}
	});

	afterEach(async () => {
		// Restore original clipboard if we saved it
		if (originalClipboard !== null) {
			const proc = Bun.spawn(["pbcopy"], {
				stdin: "pipe",
				stdout: "ignore",
				stderr: "ignore",
			});
			proc.stdin.write(originalClipboard);
			proc.stdin.end();
			await proc.exited;
		}
	});

	it("clipboard content persists after write() returns", async () => {
		const backend = createMacOSPbcopyBackend();
		const testText = `persistence-test-${Date.now()}`;

		// Write to clipboard
		const writeResult = await backend.write(testText);
		expect(writeResult).toBe(true);

		// Small delay to ensure pbcopy has finished
		await new Promise((r) => setTimeout(r, 100));

		// Read back with pbpaste
		const pasteProc = Bun.spawn(["pbpaste"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const pasteOutput = await new Response(pasteProc.stdout).text();
		await pasteProc.exited;

		expect(pasteProc.exitCode).toBe(0);
		expect(pasteOutput).toBe(testText);
	});

	it("clipboard survives multiple sequential writes", async () => {
		const backend = createMacOSPbcopyBackend();
		const texts = [
			`test-1-${Date.now()}`,
			`test-2-${Date.now()}`,
			`test-3-${Date.now()}`,
		];

		for (const text of texts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			// Verify each write
			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["pbpaste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			expect(pasteOutput).toBe(text);
		}
	});

	it("handles text with special characters", async () => {
		const backend = createMacOSPbcopyBackend();
		const specialTexts = [
			"Hello, world!",
			"Line1\nLine2\nLine3",
			"Tab\there",
			"Quotes: \"double\" and 'single'",
			"Unicode: 日本語 emoji 🎉",
			"Dollars: $100 and backticks: `code`",
		];

		for (const text of specialTexts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["pbpaste"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			// pbpaste may add a trailing newline, so trim if needed
			expect(pasteOutput.trim()).toBe(text.trim());
		}
	});

	it("handles empty string gracefully", async () => {
		const backend = createMacOSPbcopyBackend();

		// Empty string should still work (clears clipboard)
		const result = await backend.write("");
		expect(result).toBe(true);
	});

	it("pbcopy uses stdin piping (not command-line args)", async () => {
		// Note: Unlike wl-copy which can take text as argument,
		// pbcopy only reads from stdin
		const backend = createMacOSPbcopyBackend();
		const testText = "test-stdin-verification";

		const result = await backend.write(testText);
		expect(result).toBe(true);

		// Verify clipboard has the text
		await new Promise((r) => setTimeout(r, 50));
		const pasteProc = Bun.spawn(["pbpaste"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const pasteOutput = await new Response(pasteProc.stdout).text();
		await pasteProc.exited;

		expect(pasteOutput).toBe(testText);
	});
});

// Check for xsel availability (works under X11 or XWayland)
async function hasXsel(): Promise<boolean> {
	if (!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
		return false;
	}
	try {
		const proc = Bun.spawn(["which", "xsel"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		await proc.exited;
		return proc.exitCode === 0;
	} catch {
		return false;
	}
}

// Run xsel check synchronously at module load for skipIf
const xselAvailable = await hasXsel();

describe.skipIf(!xselAvailable)("Clipboard Persistence (xsel)", () => {
	let originalClipboard: string | null = null;

	beforeEach(async () => {
		// Save original clipboard content to restore later
		try {
			const proc = Bun.spawn(["xsel", "--clipboard", "--output"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			await proc.exited;
			if (proc.exitCode === 0) {
				originalClipboard = await new Response(proc.stdout).text();
			}
		} catch {
			originalClipboard = null;
		}
	});

	afterEach(async () => {
		// Restore original clipboard if we saved it
		if (originalClipboard !== null) {
			const proc = Bun.spawn(["xsel", "--clipboard", "--input"], {
				stdin: "pipe",
				stdout: "ignore",
				stderr: "ignore",
			});
			proc.stdin.write(originalClipboard);
			proc.stdin.end();
			await proc.exited;
		}
	});

	it("xsel backend writes via stdin", async () => {
		const backend = createLinuxXselBackend();
		const testText = `xsel-persistence-test-${Date.now()}`;

		const result = await backend.write(testText);
		expect(result).toBe(true);
	});

	it("clipboard content persists after write() returns", async () => {
		const backend = createLinuxXselBackend();
		const testText = `xsel-persistence-test-${Date.now()}`;

		// Write to clipboard
		const writeResult = await backend.write(testText);
		expect(writeResult).toBe(true);

		// Small delay to ensure xsel has completed
		await new Promise((r) => setTimeout(r, 100));

		// Read back with xsel
		const pasteProc = Bun.spawn(["xsel", "--clipboard", "--output"], {
			stdout: "pipe",
			stderr: "pipe",
		});
		const pasteOutput = await new Response(pasteProc.stdout).text();
		await pasteProc.exited;

		expect(pasteProc.exitCode).toBe(0);
		expect(pasteOutput.trim()).toBe(testText);
	});

	it("clipboard survives multiple sequential writes", async () => {
		const backend = createLinuxXselBackend();
		const texts = [
			`xsel-test-1-${Date.now()}`,
			`xsel-test-2-${Date.now()}`,
			`xsel-test-3-${Date.now()}`,
		];

		for (const text of texts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			// Verify each write
			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["xsel", "--clipboard", "--output"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			expect(pasteOutput.trim()).toBe(text);
		}
	});

	it("handles text with special characters", async () => {
		const backend = createLinuxXselBackend();
		const specialTexts = [
			"Hello, world!",
			"Line1\nLine2\nLine3",
			"Tab\there",
			"Quotes: \"double\" and 'single'",
			"Unicode: 日本語 emoji 🎉",
			"Dollars: $100 and backticks: `code`",
		];

		for (const text of specialTexts) {
			const result = await backend.write(text);
			expect(result).toBe(true);

			await new Promise((r) => setTimeout(r, 50));
			const pasteProc = Bun.spawn(["xsel", "--clipboard", "--output"], {
				stdout: "pipe",
				stderr: "pipe",
			});
			const pasteOutput = await new Response(pasteProc.stdout).text();
			await pasteProc.exited;

			// xsel may add a trailing newline, so trim both
			expect(pasteOutput.trim()).toBe(text.trim());
		}
	});

	it("handles empty string gracefully", async () => {
		const backend = createLinuxXselBackend();

		// Empty string should still work (clears clipboard)
		const result = await backend.write("");
		expect(result).toBe(true);
	});
});

describe("Clipboard Backend Robustness", () => {
	it("wl-copy isAvailable returns error when WAYLAND_DISPLAY not set", async () => {
		const originalWayland = process.env.WAYLAND_DISPLAY;
		delete process.env.WAYLAND_DISPLAY;

		try {
			const backend = createLinuxWlcopyBackend();
			const error = await backend.isAvailable();
			expect(error).not.toBeNull();
			expect(error).toContain("WAYLAND_DISPLAY");
		} finally {
			if (originalWayland) {
				process.env.WAYLAND_DISPLAY = originalWayland;
			}
		}
	});

	it("xsel isAvailable returns error when DISPLAY not set", async () => {
		const originalDisplay = process.env.DISPLAY;
		const originalWayland = process.env.WAYLAND_DISPLAY;
		delete process.env.DISPLAY;
		delete process.env.WAYLAND_DISPLAY;

		try {
			const backend = createLinuxXselBackend();
			const error = await backend.isAvailable();
			expect(error).not.toBeNull();
			expect(error).toContain("DISPLAY");
		} finally {
			if (originalDisplay) {
				process.env.DISPLAY = originalDisplay;
			}
			if (originalWayland) {
				process.env.WAYLAND_DISPLAY = originalWayland;
			}
		}
	});
});
