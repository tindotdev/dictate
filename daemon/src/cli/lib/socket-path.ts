import * as path from "node:path";

/**
 * Get the socket directory path.
 * Uses XDG_RUNTIME_DIR if available, otherwise falls back to ~/.local/state/dictate/
 */
export function getSocketDir(): string {
	const xdgRuntime = process.env.XDG_RUNTIME_DIR;
	if (xdgRuntime) {
		return path.join(xdgRuntime, "dictate");
	}
	return path.join(process.env.HOME ?? "/tmp", ".local", "state", "dictate");
}

/**
 * Get the full socket path.
 * Single source of truth for socket location.
 */
export function getSocketPath(): string {
	return path.join(getSocketDir(), "dictate.sock");
}
