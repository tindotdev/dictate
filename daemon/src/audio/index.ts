// ============================================================================
// Audio Backend Abstraction
// ============================================================================
// Provides a unified interface for audio capture across different platforms.

// ============================================================================
// Types
// ============================================================================

export interface AudioBackendCommand {
	/** Command to spawn (e.g., 'pw-cat', 'ffmpeg') */
	command: string;
	/** Arguments */
	args: string[];
	/** Environment variables to set */
	env?: Record<string, string>;
}

export interface AudioBackend {
	/** Human-readable name for error messages */
	name: string;
	/** Build the spawn command for audio capture */
	getCommand(): AudioBackendCommand;
	/**
	 * Validate that dependencies are present.
	 * Returns null if valid, or an error message with hint if not.
	 */
	validate(): Promise<string | null>;
}

export interface AudioBackendOptions {
	/** Sample rate in Hz (default: 24000 for OpenAI) */
	sampleRate?: number;
	/** Audio device override (optional, backend-specific) */
	device?: string;
}

// ============================================================================
// Constants
// ============================================================================

/** OpenAI Realtime API sample rate */
export const DEFAULT_SAMPLE_RATE = 24000;

// ============================================================================
// Platform Detection
// ============================================================================

export type Platform = "linux" | "darwin" | "unsupported";

export function detectPlatform(): Platform {
	const platform = process.platform;
	if (platform === "linux") return "linux";
	if (platform === "darwin") return "darwin";
	return "unsupported";
}

// ============================================================================
// Backend Factory
// ============================================================================

export { createLinuxPwcatBackend } from "./backends/linux-pwcat.js";
export { createMacOSFfmpegBackend } from "./backends/macos-ffmpeg.js";

/**
 * Create the appropriate audio backend for the current platform.
 */
export function createAudioBackend(
	options?: AudioBackendOptions,
): AudioBackend {
	const platform = detectPlatform();

	switch (platform) {
		case "linux": {
			const { createLinuxPwcatBackend } = require("./backends/linux-pwcat.js");
			return createLinuxPwcatBackend(options);
		}
		case "darwin": {
			const {
				createMacOSFfmpegBackend,
			} = require("./backends/macos-ffmpeg.js");
			return createMacOSFfmpegBackend(options);
		}
		default:
			// Return a dummy backend that fails validation
			return {
				name: "unsupported",
				getCommand: () => ({ command: "", args: [] }),
				validate: async () =>
					`Unsupported platform: ${process.platform}. Only Linux and macOS are supported.`,
			};
	}
}
