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
				// wl-copy forks into background to serve clipboard requests.
				// We use a two-step approach:
				// 1. Spawn wl-copy with --foreground to write synchronously
				// 2. The forked background process serves paste requests
				//
				// Note: We can't use detached mode because we need to wait
				// for the initial write to complete. wl-copy handles its own
				// forking internally.
				const proc = Bun.spawn(["wl-copy", "--", text], {
					stdin: "ignore",
					stdout: "ignore",
					stderr: "ignore",
				});

				// Wait for wl-copy to fork (it exits immediately after forking)
				await proc.exited;

				// wl-copy exits 0 after successfully forking
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
