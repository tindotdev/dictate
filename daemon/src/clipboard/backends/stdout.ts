// ============================================================================
// Stdout Fallback Backend
// ============================================================================
// Last resort when no clipboard tool is available. Outputs text to stdout
// with a warning to stderr.

import type { ClipboardBackend } from "../index.js";

/**
 * Create a stdout fallback "clipboard" backend.
 *
 * This backend is used as a last resort when no real clipboard tool
 * (wl-copy, xclip, xsel, pbcopy) is available. It outputs the text
 * to stdout so the user can still capture the transcription.
 */
export function createStdoutFallbackBackend(): ClipboardBackend {
	return {
		name: "stdout-fallback",

		async write(text: string): Promise<boolean> {
			// Output text to stdout (user can redirect or pipe it)
			console.log(text);
			return true;
		},

		async isAvailable(): Promise<string | null> {
			// Always available as a last resort
			return null;
		},
	};
}
