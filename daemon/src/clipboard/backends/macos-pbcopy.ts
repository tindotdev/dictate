// ============================================================================
// macOS Clipboard Backend: pbcopy
// ============================================================================

import type { ClipboardBackend } from "../index.js";

/**
 * Create a macOS clipboard backend using pbcopy.
 * pbcopy is a built-in macOS utility that reads from stdin and copies to clipboard.
 */
export function createMacOSPbcopyBackend(): ClipboardBackend {
	return {
		name: "pbcopy",

		async write(text: string): Promise<boolean> {
			try {
				const proc = Bun.spawn(["pbcopy"], {
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
			try {
				const proc = Bun.spawn(["which", "pbcopy"], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return "pbcopy not found (should be built-in on macOS)";
				}

				return null;
			} catch {
				return "pbcopy not found (should be built-in on macOS)";
			}
		},
	};
}
