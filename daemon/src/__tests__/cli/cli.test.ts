/**
 * CLI Tests
 *
 * Tests CLI tools by spawning them as subprocesses.
 * Verifies help output, version info, and basic behavior.
 */

import { describe, expect, it } from "bun:test";
import * as path from "node:path";

const CLI_DIR = path.join(import.meta.dir, "..", "..", "cli");
const DICTATE_CLI = path.join(CLI_DIR, "dictate.ts");
const DICTATECTL_CLI = path.join(CLI_DIR, "dictatectl.ts");

async function runCli(
	cliPath: string,
	args: string[],
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
	const proc = Bun.spawn(["bun", cliPath, ...args], {
		stdout: "pipe",
		stderr: "pipe",
		env: {
			...process.env,
			// Prevent auto-start from actually starting a daemon
			DICTATE_SOCKET_PATH: "/nonexistent/socket.sock",
		},
	});

	const [stdout, stderr] = await Promise.all([
		new Response(proc.stdout).text(),
		new Response(proc.stderr).text(),
	]);

	await proc.exited;

	return {
		stdout,
		stderr,
		exitCode: proc.exitCode ?? 1,
	};
}

describe("dictate CLI", () => {
	describe("--help", () => {
		it("shows help text", async () => {
			const { stdout, exitCode } = await runCli(DICTATE_CLI, ["--help"]);

			expect(exitCode).toBe(0);
			expect(stdout).toContain("dictate");
			expect(stdout).toContain("Usage:");
		});

		it("shows --clipboard option", async () => {
			const { stdout } = await runCli(DICTATE_CLI, ["--help"]);

			expect(stdout).toContain("--clipboard");
			expect(stdout).toContain("--no-clipboard");
		});

		it("shows --stdout option", async () => {
			const { stdout } = await runCli(DICTATE_CLI, ["--help"]);

			expect(stdout).toContain("--stdout");
		});

		it("shows --json option", async () => {
			const { stdout } = await runCli(DICTATE_CLI, ["--help"]);

			expect(stdout).toContain("--json");
		});

		it("shows --verbose option", async () => {
			const { stdout } = await runCli(DICTATE_CLI, ["--help"]);

			expect(stdout).toContain("--verbose");
			expect(stdout).toContain("-v");
		});

		it("shows examples", async () => {
			const { stdout } = await runCli(DICTATE_CLI, ["--help"]);

			expect(stdout).toContain("Examples:");
		});
	});

	describe("-h shorthand", () => {
		it("shows help with -h", async () => {
			const { stdout, exitCode } = await runCli(DICTATE_CLI, ["-h"]);

			expect(exitCode).toBe(0);
			expect(stdout).toContain("Usage:");
		});
	});
});

describe("dictatectl CLI", () => {
	// dictatectl is a bridge that immediately connects to the daemon.
	// It doesn't have --help or --version flags.
	// When socket is unavailable, it outputs connecting status then error.

	it("outputs JSONL format", async () => {
		const { stdout } = await runCli(DICTATECTL_CLI, []);

		// Should output JSON lines
		const lines = stdout.trim().split("\n").filter(Boolean);
		expect(lines.length).toBeGreaterThan(0);

		// First line should be parseable JSON
		const firstMsg = JSON.parse(lines[0]);
		expect(firstMsg.type).toBe("status");
	});

	it("emits connecting status on start", async () => {
		const { stdout } = await runCli(DICTATECTL_CLI, []);

		const lines = stdout.trim().split("\n").filter(Boolean);
		const msgs = lines.map((l) => JSON.parse(l));

		// Should emit connecting status
		const connectingMsg = msgs.find(
			(m) => m.type === "status" && m.state === "connecting",
		);
		expect(connectingMsg).toBeDefined();
	});
});
