import { existsSync } from "node:fs";
import { mkdir, stat } from "node:fs/promises";
import * as path from "node:path";
import { getSocketDir, getSocketPath } from "./socket-path.js";

// ============================================================================
// Types
// ============================================================================

export interface AutostartOptions {
	/** Socket path (defaults to getSocketPath()) */
	socketPath?: string;
	/** Timeout to wait for socket to appear (ms) */
	timeoutMs?: number;
	/** Daemon executable path (default: auto-discover) */
	daemonPath?: string;
}

export interface AutostartResult {
	success: boolean;
	error?: string;
	hint?: string;
	/** Error code for structured error handling (e.g., CONFIG_ERROR, DAEMON_UNAVAILABLE) */
	code?: "CONFIG_ERROR" | "DAEMON_UNAVAILABLE";
}

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_TIMEOUT_MS = 5000;
const POLL_INTERVAL_MS = 100;
const MAX_STDERR_BYTES = 4096;

// ============================================================================
// Daemon discovery
// ============================================================================

/**
 * Discover daemon executable path.
 * Order:
 * 1. DICTATED_PATH env var
 * 2. Sibling to current process (for dev: ../main.ts next to dictatectl.ts)
 * 3. Global install via PATH lookup
 */
export function discoverDaemonPath(): string | null {
	// 1. Explicit env var
	const envPath = process.env.DICTATED_PATH;
	if (envPath) {
		return envPath;
	}

	// 2. Sibling path (for dev mode and local installs)
	// dictatectl.ts is at daemon/src/cli/dictatectl.ts
	// main.ts is at daemon/src/main.ts
	const currentScript = process.argv[1];
	if (currentScript) {
		const scriptDir = path.dirname(currentScript);
		// If we're in cli/, look for ../main.ts or ../main.js
		const siblingTs = path.join(scriptDir, "..", "main.ts");
		const siblingJs = path.join(scriptDir, "..", "main.js");

		// Check synchronously for simplicity (discovery runs once at startup)
		if (existsSync(siblingTs)) {
			return siblingTs;
		}
		if (existsSync(siblingJs)) {
			return siblingJs;
		}

		// Also check for dist/main.js relative to cli/dictatectl.js
		const distMain = path.join(scriptDir, "..", "main.js");
		if (existsSync(distMain)) {
			return distMain;
		}
	}

	// 3. Global PATH lookup - dictated binary
	// Return null and let the caller handle "dictated" as command
	return null;
}

/**
 * Build the command to run the daemon.
 */
function buildDaemonCommand(daemonPath: string | null): {
	command: string;
	args: string[];
} {
	if (daemonPath) {
		// Run via bun for .ts files, node/bun for .js files
		if (daemonPath.endsWith(".ts")) {
			return { command: "bun", args: ["run", daemonPath] };
		}
		return { command: "bun", args: ["run", daemonPath] };
	}
	// Fallback: assume dictated is in PATH
	return { command: "dictated", args: [] };
}

// ============================================================================
// Socket checking
// ============================================================================

/**
 * Check if daemon is running by verifying socket file exists.
 * Note: This only checks file existence, not if daemon is actually responsive.
 */
export async function isDaemonRunning(socketPath?: string): Promise<boolean> {
	const sockPath = socketPath ?? getSocketPath();
	try {
		const stats = await stat(sockPath);
		return stats.isSocket();
	} catch {
		return false;
	}
}

/**
 * Wait for socket file to appear with polling.
 */
async function _waitForSocket(
	socketPath: string,
	timeoutMs: number,
): Promise<boolean> {
	const startTime = Date.now();

	while (Date.now() - startTime < timeoutMs) {
		if (await isDaemonRunning(socketPath)) {
			return true;
		}
		await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
	}

	return false;
}

// ============================================================================
// Auto-start logic
// ============================================================================

/**
 * Parse daemon stderr to extract structured error information.
 * Maps known patterns to error codes.
 */
function parseStderr(stderr: string): {
	code?: "CONFIG_ERROR" | "DAEMON_UNAVAILABLE";
	error: string;
	hint?: string;
} {
	const trimmed = stderr.trim();

	// CONFIG_ERROR: pattern from daemon/src/main.ts
	if (trimmed.includes("CONFIG_ERROR:")) {
		const match = trimmed.match(/CONFIG_ERROR:\s*(.+)/);
		const message = match?.[1] ?? trimmed;
		return {
			code: "CONFIG_ERROR",
			error: message,
			hint: "Check your OPENAI_API_KEY environment variable",
		};
	}

	// Configuration error from Zod validation
	if (trimmed.includes("Configuration error:")) {
		const match = trimmed.match(/Configuration error:\s*(.+)/);
		const message = match?.[1] ?? trimmed;
		return {
			code: "CONFIG_ERROR",
			error: message,
			hint: "Check your OPENAI_API_KEY environment variable",
		};
	}

	// bun: command not found
	if (
		trimmed.includes("bun: command not found") ||
		trimmed.includes("bun: not found")
	) {
		return {
			code: "DAEMON_UNAVAILABLE",
			error: "bun runtime not found",
			hint: "Install bun: curl -fsSL https://bun.sh/install | bash",
		};
	}

	// Generic stderr - return as-is
	return {
		error: trimmed || "Unknown startup error",
	};
}

/**
 * Spawn the daemon in background and wait for socket to appear.
 * Returns success/failure with actionable hints.
 * Captures stderr during startup to surface configuration errors.
 */
export async function autoStartDaemon(
	options: AutostartOptions = {},
): Promise<AutostartResult> {
	const socketPath = options.socketPath ?? getSocketPath();
	const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
	const daemonPath = options.daemonPath ?? discoverDaemonPath();

	// Ensure socket directory exists
	const socketDir = getSocketDir();
	try {
		await stat(socketDir);
	} catch {
		try {
			await mkdir(socketDir, { recursive: true, mode: 0o700 });
		} catch (err) {
			return {
				success: false,
				error: `Failed to create socket directory: ${(err as Error).message}`,
				hint: `Ensure you have write permission to ${socketDir}`,
			};
		}
	}

	// Build spawn command
	const { command, args } = buildDaemonCommand(daemonPath);

	try {
		// Spawn daemon with stderr captured for error diagnostics
		// We capture stderr during the startup window to surface config errors
		const proc = Bun.spawn([command, ...args], {
			stdio: ["ignore", "ignore", "pipe"], // Capture stderr only
		});

		// Collect stderr in background (capped to prevent memory issues)
		let stderrText = "";
		let daemonExited = false;
		let exitCode: number | null = null;

		// Read stderr asynchronously
		const stderrReader = (async () => {
			const reader = proc.stderr.getReader();
			const decoder = new TextDecoder();
			try {
				while (stderrText.length < MAX_STDERR_BYTES) {
					const { done, value } = await reader.read();
					if (done) break;
					stderrText += decoder.decode(value, { stream: true });
				}
			} catch {
				// Ignore read errors (process may have exited)
			} finally {
				reader.releaseLock();
			}
		})();

		// Track process exit
		proc.exited.then((code) => {
			daemonExited = true;
			exitCode = code;
		});

		// Don't block our exit on daemon
		proc.unref();

		// Wait for socket to appear, checking for early exit
		const startTime = Date.now();
		while (Date.now() - startTime < timeoutMs) {
			// Check if daemon exited early (config error, crash, etc.)
			if (daemonExited && exitCode !== 0) {
				// Wait a bit for stderr to be fully read
				await Promise.race([
					stderrReader,
					new Promise((r) => setTimeout(r, 100)),
				]);

				const parsed = parseStderr(stderrText);
				return {
					success: false,
					code: parsed.code ?? "DAEMON_UNAVAILABLE",
					error: parsed.error,
					hint: parsed.hint,
				};
			}

			if (await isDaemonRunning(socketPath)) {
				return { success: true };
			}

			await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
		}

		// Timeout: socket didn't appear
		// Try to get any stderr that might explain why
		await Promise.race([stderrReader, new Promise((r) => setTimeout(r, 50))]);

		if (stderrText.trim()) {
			const parsed = parseStderr(stderrText);
			return {
				success: false,
				code: parsed.code ?? "DAEMON_UNAVAILABLE",
				error: parsed.error || "Daemon started but socket did not appear",
				hint: parsed.hint ?? `Socket expected at: ${socketPath}`,
			};
		}

		return {
			success: false,
			code: "DAEMON_UNAVAILABLE",
			error: "Daemon started but socket did not appear",
			hint: `Check daemon logs. Socket expected at: ${socketPath}`,
		};
	} catch (err) {
		const errorMessage = (err as Error).message;

		// Check for common errors
		if (errorMessage.includes("ENOENT") || errorMessage.includes("not found")) {
			return {
				success: false,
				code: "DAEMON_UNAVAILABLE",
				error: `Daemon executable not found: ${command}`,
				hint: daemonPath
					? `Check that ${daemonPath} exists`
					: "Install dictate globally: bunx -p @tindotdev/dictate dictated",
			};
		}

		return {
			success: false,
			code: "DAEMON_UNAVAILABLE",
			error: `Failed to start daemon: ${errorMessage}`,
			hint: "Check that bun is installed and in PATH",
		};
	}
}

/**
 * Ensure daemon is running, auto-starting if necessary.
 * Returns true if daemon is running (was already running or started successfully).
 */
export async function ensureDaemonRunning(
	options: AutostartOptions = {},
): Promise<AutostartResult> {
	const socketPath = options.socketPath ?? getSocketPath();

	// Check if already running
	if (await isDaemonRunning(socketPath)) {
		return { success: true };
	}

	// Try to start it
	return autoStartDaemon(options);
}
