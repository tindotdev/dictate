// ============================================================================
// Linux Wayland Clipboard Backend: wl-copy
// ============================================================================

import type { ClipboardBackend } from "../index.js";

/**
 * Create a Linux Wayland clipboard backend using wl-copy.
 * wl-copy is from the wl-clipboard package.
 *
 * Install:
 *   Fedora: sudo dnf install wl-clipboard
 *   Ubuntu: sudo apt install wl-clipboard
 *   Arch: sudo pacman -S wl-clipboard
 */
export function createLinuxWlcopyBackend(): ClipboardBackend {
	return {
		name: "wl-copy",

		async write(text: string): Promise<boolean> {
			try {
				const proc = Bun.spawn(["wl-copy"], {
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
			// Check if WAYLAND_DISPLAY is set
			if (!process.env.WAYLAND_DISPLAY) {
				return "WAYLAND_DISPLAY not set (not running under Wayland)";
			}

			try {
				const proc = Bun.spawn(["which", "wl-copy"], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return "wl-copy not found. Install:\n  Fedora: sudo dnf install wl-clipboard\n  Ubuntu: sudo apt install wl-clipboard";
				}

				return null;
			} catch {
				return "wl-copy not found. Install:\n  Fedora: sudo dnf install wl-clipboard\n  Ubuntu: sudo apt install wl-clipboard";
			}
		},
	};
}
