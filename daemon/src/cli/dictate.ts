// ============================================================================
// dictate - Desktop one-shot dictation CLI
// ============================================================================
// Starts dictation, receives transcripts, copies to clipboard, exits.

import { parseArgs } from "node:util";
import { copyToClipboard, isClipboardAvailable } from "../clipboard/index.js";
import { DAEMON_VERSION, type DaemonMessage } from "../protocol.js";
import { createDaemonClient, type DaemonClient } from "./lib/daemon-client.js";
import { getSocketPath } from "./lib/socket-path.js";

// ============================================================================
// CLI Arguments
// ============================================================================

interface DictateOptions {
	clipboard: boolean;
	stdout: boolean;
	json: boolean;
	verbose: boolean;
	help: boolean;
}

function parseOptions(): DictateOptions {
	const { values } = parseArgs({
		options: {
			clipboard: { type: "boolean", default: true },
			"no-clipboard": { type: "boolean", default: false },
			stdout: { type: "boolean", default: false },
			json: { type: "boolean", default: false },
			verbose: { type: "boolean", short: "v", default: false },
			help: { type: "boolean", short: "h", default: false },
		},
		allowPositionals: false,
	});

	return {
		clipboard: values.clipboard && !values["no-clipboard"],
		stdout: values.stdout ?? false,
		json: values.json ?? false,
		verbose: values.verbose ?? false,
		help: values.help ?? false,
	};
}

function printHelp(): void {
	console.log(`
dictate - One-shot voice dictation CLI

Usage: dictate [options]

Options:
  --clipboard       Copy final transcript to clipboard (default)
  --no-clipboard    Don't copy to clipboard
  --stdout          Print partials and final to stdout
  --json            Output JSONL format (for integration)
  -v, --verbose     Show debug output
  -h, --help        Show this help

Examples:
  dictate                    # Dictate, copy to clipboard
  dictate --stdout           # Print transcript to stdout
  dictate --no-clipboard     # Just print, don't copy
  dictate --json             # JSONL output for scripts

Press Ctrl+C to stop listening and wait for final transcript.
`);
}

// ============================================================================
// Output helpers
// ============================================================================

function output(opts: DictateOptions, msg: DaemonMessage): void {
	if (opts.json) {
		console.log(JSON.stringify(msg));
		return;
	}

	switch (msg.type) {
		case "status":
			if (opts.verbose) {
				console.error(`[status] ${msg.state}`);
			}
			break;
		case "partial_transcript":
			if (opts.stdout || opts.verbose) {
				// Clear line and reprint partial (for terminal UX)
				process.stdout.write(`\r\x1b[K${msg.text}`);
			}
			break;
		case "final_transcript":
			if (opts.stdout || opts.verbose) {
				// Clear the partial line, print final with newline
				process.stdout.write(`\r\x1b[K${msg.text}\n`);
			}
			break;
		case "error":
			console.error(`[error] ${msg.code}: ${msg.message}`);
			if (msg.hint) {
				console.error(`  Hint: ${msg.hint}`);
			}
			break;
		case "speech_started":
			if (opts.verbose) {
				console.error("[speech] started");
			}
			break;
		case "speech_stopped":
			if (opts.verbose) {
				console.error("[speech] stopped");
			}
			break;
		case "debug":
			if (opts.verbose) {
				console.error(`[debug] ${msg.message}`);
			}
			break;
	}
}

// ============================================================================
// Main
// ============================================================================

async function main(): Promise<void> {
	const opts = parseOptions();

	if (opts.help) {
		printHelp();
		process.exit(0);
	}

	// Check clipboard availability if we plan to use it
	if (opts.clipboard) {
		const clipboardError = await isClipboardAvailable();
		if (clipboardError) {
			console.error(`[warn] Clipboard unavailable: ${clipboardError}`);
			console.error("[warn] Transcript will only be printed to stdout");
			opts.clipboard = false;
			opts.stdout = true;
		}
	}

	const socketPath = getSocketPath();
	let client: DaemonClient | null = null;
	let stopping = false;
	let finalTranscript: string | null = null;
	let waitingForFinal = false;

	// Create daemon client
	client = createDaemonClient({
		socketPath,
		autoStart: true,
		reconnect: false, // One-shot: don't reconnect on disconnect

		onConnect: () => {
			if (opts.verbose) {
				console.error("[connected] Daemon connected");
			}
			// Send initialize
			client?.send({
				type: "initialize",
				version: DAEMON_VERSION,
			});
		},

		onMessage: async (msg: DaemonMessage) => {
			output(opts, msg);

			switch (msg.type) {
				case "initialized":
					// Start listening
					if (opts.verbose) {
						console.error("[listening] Starting dictation...");
					}
					client?.send({ type: "start_listening" });
					break;

				case "final_transcript":
					finalTranscript = msg.text;

					// Copy to clipboard if enabled
					if (opts.clipboard && msg.text.trim()) {
						const success = await copyToClipboard(msg.text);
						if (opts.verbose) {
							if (success) {
								console.error("[clipboard] Copied to clipboard");
							} else {
								console.error("[clipboard] Failed to copy");
							}
						}
					}

					// If we were waiting for final (Ctrl+C), exit now
					if (waitingForFinal) {
						client?.disconnect();
						process.exit(0);
					}
					break;

				case "status":
					// If we transitioned to idle after stopping, we're done
					if (msg.state === "idle" && stopping) {
						client?.disconnect();
						process.exit(0);
					}
					break;

				case "error":
					if (!msg.recoverable) {
						client?.disconnect();
						process.exit(1);
					}
					break;
			}
		},

		onDisconnect: () => {
			if (opts.verbose) {
				console.error("[disconnected] Daemon disconnected");
			}
			// If we didn't get a final transcript, exit with error
			if (!stopping && finalTranscript === null) {
				console.error("[error] Disconnected before receiving transcript");
				process.exit(1);
			}
			process.exit(0);
		},

		onError: (err) => {
			console.error(`[error] ${err.message}`);
			process.exit(1);
		},
	});

	// Handle Ctrl+C: stop listening, wait for final transcript
	const handleSignal = () => {
		if (stopping) {
			// Second Ctrl+C: force exit
			if (opts.verbose) {
				console.error("\n[exit] Force quit");
			}
			client?.disconnect();
			process.exit(130);
		}

		stopping = true;
		waitingForFinal = true;

		if (opts.verbose) {
			console.error("\n[stopping] Waiting for final transcript...");
		}

		client?.send({ type: "stop_listening" });

		// Timeout: if no final transcript after 5 seconds, exit anyway
		setTimeout(() => {
			if (waitingForFinal) {
				if (opts.verbose) {
					console.error("[timeout] No final transcript received");
				}
				client?.disconnect();
				process.exit(0);
			}
		}, 5000);
	};

	process.on("SIGINT", handleSignal);
	process.on("SIGTERM", handleSignal);

	// Connect to daemon
	try {
		await client.connect();
	} catch (err) {
		console.error(`[error] Failed to connect: ${(err as Error).message}`);
		process.exit(1);
	}
}

main().catch((err) => {
	console.error(`[fatal] ${err.message}`);
	process.exit(1);
});
