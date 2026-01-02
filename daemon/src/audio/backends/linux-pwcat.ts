// ============================================================================
// Linux Audio Backend: PipeWire (pw-cat)
// ============================================================================

import type { AudioBackend, AudioBackendOptions } from "../index.js";
import { DEFAULT_SAMPLE_RATE } from "../index.js";

export interface LinuxPwcatOptions extends AudioBackendOptions {
	/** Path to pw-cat binary (default: 'pw-cat') */
	pwCatPath?: string;
}

/**
 * Create a Linux audio backend using PipeWire's pw-cat.
 */
export function createLinuxPwcatBackend(
	options?: LinuxPwcatOptions,
): AudioBackend {
	const sampleRate = options?.sampleRate ?? DEFAULT_SAMPLE_RATE;
	const pwCatPath = options?.pwCatPath ?? "pw-cat";

	return {
		name: "pw-cat (PipeWire)",

		getCommand() {
			const args = [
				"--record",
				"--raw",
				`--rate=${sampleRate}`,
				"--channels=1",
				"--format=s16",
				"-", // Output to stdout
			];

			// Add device target if specified
			if (options?.device) {
				args.push(`--target=${options.device}`);
			}

			return {
				command: pwCatPath,
				args,
			};
		},

		async validate() {
			try {
				const proc = Bun.spawn(["which", pwCatPath], {
					stdout: "pipe",
					stderr: "pipe",
				});
				await proc.exited;

				if (proc.exitCode !== 0) {
					return `pw-cat not found. Install PipeWire utilities:\n  Fedora: sudo dnf install pipewire-utils\n  Ubuntu: sudo apt install pipewire`;
				}

				return null;
			} catch {
				return `pw-cat not found. Install PipeWire utilities:\n  Fedora: sudo dnf install pipewire-utils\n  Ubuntu: sudo apt install pipewire`;
			}
		},
	};
}
