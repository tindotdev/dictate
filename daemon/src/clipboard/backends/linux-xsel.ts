// ============================================================================
// Linux X11 Clipboard Backend: xsel
// ============================================================================

import type { ClipboardBackend } from "../index.js";

/**
 * Create a Linux X11 clipboard backend using xsel.
 *
 * Uses the clipboard selection (not primary selection) for better
 * compatibility with most applications. This backend is a fallback
 * when xclip is not available.
 *
 * Install:
 *   Fedora: sudo dnf install xsel
 *   Ubuntu: sudo apt install xsel
 *   Arch: sudo pacman -S xsel
 */
export function createLinuxXselBackend(): ClipboardBackend {
	return {
		name: "xsel",

		async write(text: string): Promise<boolean> {
			try {
				const proc = Bun.spawn(["xsel", "--clipboard", "--input"], {
					stdin: "pipe",
					stdout: "pipe",
					stderr: "pipe",
				});

				// Write text to stdin (FileSink API in Bun)
				proc.stdin.write(text);
				proc.stdin.end();

				// Wait for process to exit
				await proc.exited;

				return proc.exitCode === 0;
			} catch {
				return false;
			}
		},

		async isAvailable(): Promise<string | null> {
			// Check if DISPLAY is set (required for X11)
			if (!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
				return "DISPLAY not set (not running under X11 or Wayland)";
			}

			try {
				const proc = Bun.spawn(["which", "xsel"], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return "xsel not found. Install:\n  Fedora: sudo dnf install xsel\n  Ubuntu: sudo apt install xsel";
				}

				return null;
			} catch {
				return "xsel not found. Install:\n  Fedora: sudo dnf install xsel\n  Ubuntu: sudo apt install xsel";
			}
		},
	};
}
