import { stat } from "node:fs/promises";
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
}

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_TIMEOUT_MS = 5000;
const POLL_INTERVAL_MS = 100;

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
		const fs = require("node:fs");
		if (fs.existsSync(siblingTs)) {
			return siblingTs;
		}
		if (fs.existsSync(siblingJs)) {
			return siblingJs;
		}

		// Also check for dist/main.js relative to cli/dictatectl.js
		const distMain = path.join(scriptDir, "..", "main.js");
		if (fs.existsSync(distMain)) {
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
async function waitForSocket(
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
 * Spawn the daemon in background and wait for socket to appear.
 * Returns success/failure with actionable hints.
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
		const { mkdir } = await import("node:fs/promises");
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
		// Spawn daemon in background (detached, no stdio)
		const proc = Bun.spawn([command, ...args], {
			stdio: ["ignore", "ignore", "ignore"],
			// Note: Bun doesn't have 'detached' like Node, but the process
			// will continue running after parent exits
		});

		// Don't wait for process to exit - it's a daemon
		// Just unref it so it doesn't block our exit
		proc.unref();

		// Wait for socket to appear
		const socketAppeared = await waitForSocket(socketPath, timeoutMs);

		if (!socketAppeared) {
			return {
				success: false,
				error: "Daemon started but socket did not appear",
				hint: `Check daemon logs. Socket expected at: ${socketPath}`,
			};
		}

		return { success: true };
	} catch (err) {
		const errorMessage = (err as Error).message;

		// Check for common errors
		if (errorMessage.includes("ENOENT") || errorMessage.includes("not found")) {
			return {
				success: false,
				error: `Daemon executable not found: ${command}`,
				hint: daemonPath
					? `Check that ${daemonPath} exists`
					: "Install dictate globally: bunx -p @tindotdev/dictate dictated",
			};
		}

		return {
			success: false,
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
