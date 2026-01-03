/**
 * Clipboard Fallback Chain Tests
 *
 * Tests the clipboard backend selection and fallback behavior.
 * These tests run in subprocesses to avoid cache pollution since
 * getClipboardBackend() caches the selected backend.
 */

import { describe, expect, it } from "bun:test";
import path from "node:path";

const hasWayland = !!process.env.WAYLAND_DISPLAY;
const hasX11 = !!process.env.DISPLAY;

// Get bun executable path to preserve in modified PATH tests
const bunPath = Bun.which("bun") ?? "bun";
const bunDir = path.dirname(bunPath);

// Helper to run clipboard test in subprocess with custom environment
async function runClipboardTest(
	code: string,
	env: Record<string, string | undefined> = {},
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
	const testCode = `
		import { isClipboardAvailable, copyToClipboard } from './src/clipboard/index.js';
		${code}
	`;

	// If PATH is being overridden, ensure bun is still accessible
	const finalEnv = { ...process.env, ...env };
	if (env.PATH !== undefined && !env.PATH.includes(bunDir)) {
		finalEnv.PATH = `${bunDir}:${env.PATH}`;
	}

	const proc = Bun.spawn([bunPath, "-e", testCode], {
		cwd: path.resolve(import.meta.dir, "../../../"),
		stdout: "pipe",
		stderr: "pipe",
		env: finalEnv,
	});

	const [stdout, stderr] = await Promise.all([
		new Response(proc.stdout).text(),
		new Response(proc.stderr).text(),
	]);

	await proc.exited;

	return {
		stdout: stdout.trim(),
		stderr: stderr.trim(),
		exitCode: proc.exitCode ?? 1,
	};
}

describe("Fallback Chain - isClipboardAvailable()", () => {
	it("returns null when clipboard is available", async () => {
		// This test only makes sense if we have a display
		if (!hasWayland && !hasX11) {
			return;
		}

		const result = await runClipboardTest(`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`);

		expect(result.exitCode).toBe(0);
		const { error } = JSON.parse(result.stdout);

		// Should be null if wl-copy/xclip/xsel available, or contain fallback message
		if (error !== null) {
			expect(error).toContain("fallback");
		}
	});

	it("returns fallback message when no clipboard tools in PATH", async () => {
		const result = await runClipboardTest(
			`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`,
			{
				// Use empty temp dir as PATH to hide all clipboard tools
				PATH: "/nonexistent",
				WAYLAND_DISPLAY: "wayland-1",
				DISPLAY: ":0",
			},
		);

		expect(result.exitCode).toBe(0);
		const { error } = JSON.parse(result.stdout);

		expect(error).not.toBeNull();
		expect(error).toContain("fallback");
	});

	it.skipIf(process.platform === "darwin")(
		"reports Wayland install hint on linux-wayland without tools",
		async () => {
			const result = await runClipboardTest(
				`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: "wayland-1",
					DISPLAY: undefined,
				},
			);

			expect(result.exitCode).toBe(0);
			const { error } = JSON.parse(result.stdout);

			expect(error).toContain("wl-copy");
			expect(error).toContain("xclip");
			expect(error).toContain("xsel");
		},
	);

	it.skipIf(process.platform === "darwin")(
		"reports X11 install hint on linux-x11 without tools",
		async () => {
			const result = await runClipboardTest(
				`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: undefined,
					DISPLAY: ":0",
				},
			);

			expect(result.exitCode).toBe(0);
			const { error } = JSON.parse(result.stdout);

			expect(error).toContain("xclip");
			expect(error).toContain("xsel");
		},
	);
});

describe("Fallback Chain - copyToClipboard()", () => {
	it("returns true even with stdout fallback", async () => {
		const result = await runClipboardTest(
			`
			// Suppress console.log from stdout fallback backend
			const originalLog = console.log;
			let capturedOutput = '';
			console.log = (msg) => { capturedOutput = msg; };

			const success = await copyToClipboard('test');

			console.log = originalLog;
			console.log(JSON.stringify({ success, capturedOutput }));
		`,
			{
				PATH: "/nonexistent",
				WAYLAND_DISPLAY: "wayland-1",
			},
		);

		expect(result.exitCode).toBe(0);
		const parsed = JSON.parse(result.stdout);
		expect(parsed.success).toBe(true);
		// stdout fallback should have printed our text
		expect(parsed.capturedOutput).toBe("test");
	});

	it("prints warning when falling back to stdout", async () => {
		const result = await runClipboardTest(
			`
			// isClipboardAvailable triggers the backend selection + warning
			await isClipboardAvailable();
		`,
			{
				PATH: "/nonexistent",
				WAYLAND_DISPLAY: "wayland-1",
			},
		);

		// Warning goes to stderr
		expect(result.stderr).toContain("Falling back to stdout");
	});
});

describe.skipIf(!hasWayland)("Fallback Chain - Wayland Priority", () => {
	it("uses wl-copy when available", async () => {
		const result = await runClipboardTest(`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`);

		expect(result.exitCode).toBe(0);
		const { error } = JSON.parse(result.stdout);

		// wl-copy should be available on Wayland, so no error
		expect(error).toBeNull();
	});
});

describe("Fallback Chain - Backend Selection Order", () => {
	// These tests verify the fallback order by checking which backend gets selected

	it.skipIf(process.platform === "darwin")(
		"linux-wayland: wl-copy > xclip > xsel > stdout",
		async () => {
			// Test with all tools missing
			const result = await runClipboardTest(
				`
			const error = await isClipboardAvailable();
			// Error message should mention all tools tried
			console.log(JSON.stringify({ error }));
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: "wayland-1",
					DISPLAY: undefined,
				},
			);

			const { error } = JSON.parse(result.stdout);

			// Should try wl-copy first, then mention alternatives
			expect(error).toContain("wl-copy");
		},
	);

	it.skipIf(process.platform === "darwin")(
		"linux-x11: xclip > xsel > stdout",
		async () => {
			const result = await runClipboardTest(
				`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: undefined,
					DISPLAY: ":0",
				},
			);

			const { error } = JSON.parse(result.stdout);

			// Should mention xclip and xsel
			expect(error).toContain("xclip");
			expect(error).toContain("xsel");
		},
	);

	it("darwin: pbcopy > stdout", async () => {
		// Can only really test this on macOS, but we can at least verify
		// the code path doesn't crash
		if (process.platform !== "darwin") {
			return;
		}

		const result = await runClipboardTest(`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`);

		expect(result.exitCode).toBe(0);
		const { error } = JSON.parse(result.stdout);
		expect(error).toBeNull(); // pbcopy always available on macOS
	});

	it("unsupported platform falls back to stdout", async () => {
		// Simulate unsupported platform by clearing all display vars
		const result = await runClipboardTest(
			`
			const error = await isClipboardAvailable();
			console.log(JSON.stringify({ error }));
		`,
			{
				PATH: "/nonexistent",
				WAYLAND_DISPLAY: undefined,
				DISPLAY: undefined,
			},
		);

		expect(result.exitCode).toBe(0);
		const { error } = JSON.parse(result.stdout);

		// Should fall back to stdout
		expect(error).toContain("fallback");
	});
});

describe.skipIf(process.platform === "darwin")(
	"Fallback Chain - Warning Messages",
	() => {
		it("warns about missing wl-copy on Wayland", async () => {
			const result = await runClipboardTest(
				`
			await isClipboardAvailable();
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: "wayland-1",
				},
			);

			expect(result.stderr).toContain("Wayland clipboard unavailable");
			expect(result.stderr).toContain("Falling back to stdout");
		});

		it("warns about missing xclip on X11", async () => {
			const result = await runClipboardTest(
				`
			await isClipboardAvailable();
		`,
				{
					PATH: "/nonexistent",
					WAYLAND_DISPLAY: undefined,
					DISPLAY: ":0",
				},
			);

			expect(result.stderr).toContain("X11 clipboard unavailable");
			expect(result.stderr).toContain("Falling back to stdout");
		});
	},
);
