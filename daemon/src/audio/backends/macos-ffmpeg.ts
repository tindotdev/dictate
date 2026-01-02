// ============================================================================
// macOS Audio Backend: FFmpeg (avfoundation)
// ============================================================================

import type { AudioBackend, AudioBackendOptions } from "../index.js";
import { DEFAULT_SAMPLE_RATE } from "../index.js";

export interface MacOSFfmpegOptions extends AudioBackendOptions {
	/** Path to ffmpeg binary (default: 'ffmpeg') */
	ffmpegPath?: string;
}

/**
 * Create a macOS audio backend using FFmpeg with AVFoundation.
 *
 * The default device ":default" captures from the system default microphone.
 * To list available devices:
 *   ffmpeg -f avfoundation -list_devices true -i ""
 *
 * Device format: "[video_device_index]:[audio_device_index]"
 * For audio only, use ":0" or ":default"
 */
export function createMacOSFfmpegBackend(
	options?: MacOSFfmpegOptions,
): AudioBackend {
	const sampleRate = options?.sampleRate ?? DEFAULT_SAMPLE_RATE;
	const ffmpegPath = options?.ffmpegPath ?? "ffmpeg";
	// Default to ":default" which selects the system default audio input
	const device = options?.device ?? ":default";

	return {
		name: "ffmpeg (avfoundation)",

		getCommand() {
			return {
				command: ffmpegPath,
				args: [
					"-f",
					"avfoundation", // macOS audio/video capture
					"-i",
					device, // Input device
					"-ar",
					String(sampleRate), // Sample rate
					"-ac",
					"1", // Mono
					"-f",
					"s16le", // 16-bit signed little-endian PCM
					"-", // Output to stdout
				],
			};
		},

		async validate() {
			try {
				const proc = Bun.spawn(["which", ffmpegPath], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return `ffmpeg not found. Install with Homebrew:\n  brew install ffmpeg`;
				}

				// Optionally verify avfoundation support
				const versionProc = Bun.spawn(
					[ffmpegPath, "-hide_banner", "-demuxers"],
					{
						stdout: "pipe",
						stderr: "pipe",
					},
				);
				await versionProc.exited;

				const stdout = await new Response(versionProc.stdout).text();
				if (!stdout.includes("avfoundation")) {
					return `ffmpeg is installed but lacks avfoundation support. Reinstall with:\n  brew reinstall ffmpeg`;
				}

				return null;
			} catch {
				return `ffmpeg not found. Install with Homebrew:\n  brew install ffmpeg`;
			}
		},
	};
}
