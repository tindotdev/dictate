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
import { createLinuxXselBackend } from "./backends/linux-xsel.js";
import { createMacOSPbcopyBackend } from "./backends/macos-pbcopy.js";
import { createStdoutFallbackBackend } from "./backends/stdout.js";

export { createLinuxWlcopyBackend } from "./backends/linux-wlcopy.js";
export { createLinuxXclipBackend } from "./backends/linux-xclip.js";
export { createLinuxXselBackend } from "./backends/linux-xsel.js";
export { createMacOSPbcopyBackend } from "./backends/macos-pbcopy.js";
export { createStdoutFallbackBackend } from "./backends/stdout.js";

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
			// pbcopy should always be available on macOS, but fall back to stdout just in case
			console.warn(`[clipboard] macOS pbcopy unavailable: ${error}`);
			console.warn("[clipboard] Falling back to stdout");
			const stdoutBackend = createStdoutFallbackBackend();
			cachedBackend = stdoutBackend;
			return stdoutBackend;
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

			// Fall back to xsel (might work under XWayland)
			const xselBackend = createLinuxXselBackend();
			const xselError = await xselBackend.isAvailable();
			if (!xselError) {
				cachedBackend = xselBackend;
				return xselBackend;
			}

			// Final fallback: stdout
			console.warn(
				`[clipboard] Wayland clipboard unavailable. Install wl-clipboard:\n  ${wlError}`,
			);
			console.warn("[clipboard] Falling back to stdout");
			const stdoutBackend = createStdoutFallbackBackend();
			cachedBackend = stdoutBackend;
			return stdoutBackend;
		}

		case "linux-x11": {
			// Try xclip first (more common)
			const xclipBackend = createLinuxXclipBackend();
			const xclipError = await xclipBackend.isAvailable();
			if (!xclipError) {
				cachedBackend = xclipBackend;
				return xclipBackend;
			}

			// Fall back to xsel
			const xselBackend = createLinuxXselBackend();
			const xselError = await xselBackend.isAvailable();
			if (!xselError) {
				cachedBackend = xselBackend;
				return xselBackend;
			}

			// Final fallback: stdout
			console.warn(
				`[clipboard] X11 clipboard unavailable. Install xclip or xsel:\n  ${xclipError}`,
			);
			console.warn("[clipboard] Falling back to stdout");
			const stdoutBackend = createStdoutFallbackBackend();
			cachedBackend = stdoutBackend;
			return stdoutBackend;
		}

		default: {
			console.warn(`[clipboard] Unsupported platform: ${process.platform}`);
			console.warn("[clipboard] Falling back to stdout");
			const stdoutBackend = createStdoutFallbackBackend();
			cachedBackend = stdoutBackend;
			return stdoutBackend;
		}
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
 * Returns null if a real clipboard backend is available.
 * Returns an error message if only the stdout fallback is available.
 */
export async function isClipboardAvailable(): Promise<string | null> {
	const backend = await getClipboardBackend();

	// This should not happen now since we always return at least stdout fallback
	if (!backend) {
		return `Clipboard not supported on ${process.platform}`;
	}

	// If we got the stdout fallback, report it as "unavailable" but functional
	if (backend.name === "stdout-fallback") {
		const platform = detectPlatform();
		switch (platform) {
			case "darwin":
				return "pbcopy not found (should be built-in on macOS). Using stdout fallback.";
			case "linux-wayland":
				return "wl-copy/xclip/xsel not found. Using stdout fallback. Install: sudo apt install wl-clipboard";
			case "linux-x11":
				return "xclip/xsel not found. Using stdout fallback. Install: sudo apt install xclip";
			default:
				return `Clipboard not supported on ${process.platform}. Using stdout fallback.`;
		}
	}

	return null;
}
