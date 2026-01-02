// ============================================================================
// Clipboard Abstraction
// ============================================================================
// Provides a unified interface for clipboard writing across platforms.

// ============================================================================
// Types
// ============================================================================

export interface ClipboardBackend {
	/** Human-readable name for error messages */
	name: string;
	/** Write text to clipboard. Returns true on success. */
	write(text: string): Promise<boolean>;
	/** Check if this backend is available. Returns null if available, error message if not. */
	isAvailable(): Promise<string | null>;
}

// ============================================================================
// Platform Detection
// ============================================================================

type Platform = "linux-wayland" | "linux-x11" | "darwin" | "unsupported";

function detectPlatform(): Platform {
	if (process.platform === "darwin") {
		return "darwin";
	}

	if (process.platform === "linux") {
		// Check for Wayland
		if (process.env.WAYLAND_DISPLAY) {
			return "linux-wayland";
		}
		// Check for X11
		if (process.env.DISPLAY) {
			return "linux-x11";
		}
		// Fallback to X11 (might be a headless session or weird setup)
		return "linux-x11";
	}

	return "unsupported";
}

// ============================================================================
// Backend Implementations
// ============================================================================

import { createLinuxWlcopyBackend } from "./backends/linux-wlcopy.js";
import { createLinuxXclipBackend } from "./backends/linux-xclip.js";
import { createMacOSPbcopyBackend } from "./backends/macos-pbcopy.js";

export { createLinuxWlcopyBackend } from "./backends/linux-wlcopy.js";
export { createLinuxXclipBackend } from "./backends/linux-xclip.js";
export { createMacOSPbcopyBackend } from "./backends/macos-pbcopy.js";

// ============================================================================
// Auto-detecting Clipboard Function
// ============================================================================

let cachedBackend: ClipboardBackend | null = null;

/**
 * Get the appropriate clipboard backend for the current platform.
 * The backend is cached after first detection.
 */
async function getClipboardBackend(): Promise<ClipboardBackend | null> {
	if (cachedBackend) {
		return cachedBackend;
	}

	const platform = detectPlatform();

	switch (platform) {
		case "darwin": {
			const backend = createMacOSPbcopyBackend();
			const error = await backend.isAvailable();
			if (!error) {
				cachedBackend = backend;
				return backend;
			}
			console.warn(`[clipboard] macOS pbcopy unavailable: ${error}`);
			return null;
		}

		case "linux-wayland": {
			// Try wl-copy first
			const wlBackend = createLinuxWlcopyBackend();
			const wlError = await wlBackend.isAvailable();
			if (!wlError) {
				cachedBackend = wlBackend;
				return wlBackend;
			}

			// Fall back to xclip (might work under XWayland)
			const xclipBackend = createLinuxXclipBackend();
			const xclipError = await xclipBackend.isAvailable();
			if (!xclipError) {
				cachedBackend = xclipBackend;
				return xclipBackend;
			}

			console.warn(
				`[clipboard] Wayland clipboard unavailable. Install wl-clipboard:\n  ${wlError}`,
			);
			return null;
		}

		case "linux-x11": {
			// Try xclip first (more common)
			const xclipBackend = createLinuxXclipBackend();
			const xclipError = await xclipBackend.isAvailable();
			if (!xclipError) {
				cachedBackend = xclipBackend;
				return xclipBackend;
			}

			console.warn(
				`[clipboard] X11 clipboard unavailable. Install xclip:\n  ${xclipError}`,
			);
			return null;
		}

		default:
			console.warn(`[clipboard] Unsupported platform: ${process.platform}`);
			return null;
	}
}

/**
 * Copy text to the system clipboard.
 * Returns true on success, false on failure.
 *
 * On failure, a warning is printed to stderr but no exception is thrown.
 */
export async function copyToClipboard(text: string): Promise<boolean> {
	const backend = await getClipboardBackend();

	if (!backend) {
		return false;
	}

	try {
		return await backend.write(text);
	} catch (err) {
		console.warn(
			`[clipboard] Failed to copy to clipboard: ${(err as Error).message}`,
		);
		return false;
	}
}

/**
 * Check if clipboard functionality is available on this system.
 * Returns null if available, or an error message if not.
 */
export async function isClipboardAvailable(): Promise<string | null> {
	const backend = await getClipboardBackend();

	if (!backend) {
		const platform = detectPlatform();
		switch (platform) {
			case "darwin":
				return "pbcopy not found (should be built-in on macOS)";
			case "linux-wayland":
				return "wl-copy not found. Install: sudo apt install wl-clipboard";
			case "linux-x11":
				return "xclip not found. Install: sudo apt install xclip";
			default:
				return `Clipboard not supported on ${process.platform}`;
		}
	}

	return null;
}
