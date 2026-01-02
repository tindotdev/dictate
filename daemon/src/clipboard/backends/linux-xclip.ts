// ============================================================================
// Linux X11 Clipboard Backend: xclip
// ============================================================================

import type { ClipboardBackend } from "../index.js";

/**
 * Create a Linux X11 clipboard backend using xclip.
 *
 * Uses the clipboard selection (not primary selection) for better
 * compatibility with most applications.
 *
 * Install:
 *   Fedora: sudo dnf install xclip
 *   Ubuntu: sudo apt install xclip
 *   Arch: sudo pacman -S xclip
 */
export function createLinuxXclipBackend(): ClipboardBackend {
	return {
		name: "xclip",

		async write(text: string): Promise<boolean> {
			try {
				const proc = Bun.spawn(["xclip", "-selection", "clipboard"], {
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
				const proc = Bun.spawn(["which", "xclip"], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return "xclip not found. Install:\n  Fedora: sudo dnf install xclip\n  Ubuntu: sudo apt install xclip";
				}

				return null;
			} catch {
				return "xclip not found. Install:\n  Fedora: sudo dnf install xclip\n  Ubuntu: sudo apt install xclip";
			}
		},
	};
}
