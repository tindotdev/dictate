import * as path from "node:path";
import { type Config, loadConfig } from "./config.js";
import {
	createIdleExitPolicy,
	type IdleExitPolicy,
	loadIdleTimeoutFromEnv,
} from "./lifecycle/idle-exit.js";
import {
	DAEMON_VERSION,
	type DaemonMessage,
	type DaemonState,
	type ErrorCode,
} from "./protocol.js";
import { createSocketServer, type SocketServer } from "./server.js";
import {
	createStateMachine,
	type DaemonStateMachine,
} from "./state-machine.js";
import {
	type AudioSupervisor,
	createAudioSupervisor,
} from "./supervisors/audio.js";
import {
	createNetworkSupervisor,
	type NetworkSupervisor,
} from "./supervisors/network.js";

// ============================================================================
// Load environment variables
// ============================================================================

const daemonDir = path.dirname(Bun.main);
const envPath = path.join(daemonDir, "..", ".env");
try {
	const envFile = Bun.file(envPath);
	if (await envFile.exists()) {
		const content = await envFile.text();
		for (const line of content.split("\n")) {
			const trimmed = line.trim();
			if (!trimmed || trimmed.startsWith("#")) continue;
			const eqIndex = trimmed.indexOf("=");
			if (eqIndex === -1) continue;
			const key = trimmed.slice(0, eqIndex).trim();
			const value = trimmed
				.slice(eqIndex + 1)
				.trim()
				.replace(/^["']|["']$/g, "");
			if (!process.env[key]) {
				process.env[key] = value;
			}
		}
	}
} catch {
	// .env file doesn't exist or can't be read, that's fine
}

// ============================================================================
// Load configuration
// ============================================================================

let config: Config;
try {
	config = loadConfig();
} catch (err) {
	console.error(`CONFIG_ERROR: ${(err as Error).message}`);
	process.exit(1);
}

// ============================================================================
// Debug logging
// ============================================================================

const DEBUG = process.env.DEBUG === "1";

function debug(message: string): void {
	if (DEBUG) {
		console.error(`[daemon] ${message}`);
	}
}

// ============================================================================
// Initialize components
// ============================================================================

const server: SocketServer = createSocketServer();
const audio: AudioSupervisor = createAudioSupervisor();
const network: NetworkSupervisor = createNetworkSupervisor({ config });
const stateMachine: DaemonStateMachine = createStateMachine();

// Track accumulated text per item_id (for delta -> full text conversion)
const itemTexts = new Map<string, string>();

// Session ownership: only the client that started listening receives transcripts
let sessionOwner: string | null = null;

// Idle exit policy: exit daemon after timeout when no clients and not listening
const idleTimeoutMs = loadIdleTimeoutFromEnv();
const idleExitPolicy: IdleExitPolicy = createIdleExitPolicy({
	timeoutMs: idleTimeoutMs,
	isIdle: () =>
		server.getClientCount() === 0 && stateMachine.getState() === "idle",
	onExit: () => {
		debug(`Idle timeout (${idleTimeoutMs}ms) - shutting down`);
		shutdown();
	},
});

if (idleTimeoutMs > 0) {
	debug(`Idle exit enabled: ${idleTimeoutMs}ms`);
} else {
	debug("Idle exit disabled");
}

// ============================================================================
// Helper: Broadcast status to all clients
// ============================================================================

function broadcastStatus(message?: string): void {
	const state = stateMachine.getState();
	const audioOk = audio.isRunning();
	const wsOk = network.isConnected();

	server.broadcast({
		type: "status",
		state,
		audio_ok: audioOk,
		ws_ok: wsOk,
		message,
	});
}

function broadcastError(
	code: ErrorCode,
	message: string,
	recoverable: boolean,
	hint?: string,
): void {
	server.broadcast({
		type: "error",
		code,
		message,
		recoverable,
		hint,
	});
}

function sendToOwner(msg: DaemonMessage): void {
	if (sessionOwner) {
		server.send(sessionOwner, msg);
	}
}

function sendErrorToClient(
	clientId: string,
	code: ErrorCode,
	message: string,
	recoverable: boolean,
	hint?: string,
): void {
	server.send(clientId, { type: "error", code, message, recoverable, hint });
}

// ============================================================================
// State machine event handlers
// ============================================================================

stateMachine.on("transition", (from: DaemonState, to: DaemonState) => {
	debug(`State: ${from} -> ${to}`);

	// Clear session ownership when returning to idle or entering error state
	// (Error state clears ownership to avoid stale SESSION_BUSY rejections)
	if ((to === "idle" || to === "error") && sessionOwner !== null) {
		debug(`Clearing session owner: ${sessionOwner}`);
		sessionOwner = null;
	}

	// Idle exit policy: track dictation start/stop
	if (to === "listening" || to === "audio_starting") {
		idleExitPolicy.onDictationStart();
	} else if (to === "idle" || to === "error") {
		idleExitPolicy.onDictationStop();
	}

	broadcastStatus();
});

stateMachine.on("error", (message: string) => {
	debug(`State machine error: ${message}`);
	broadcastError("INTERNAL_ERROR", message, false);
});

// ============================================================================
// Audio supervisor event handlers
// ============================================================================

audio.on("started", () => {
	debug("Audio capture started");
	stateMachine.transition({ type: "AUDIO_READY" });
});

audio.on("stopped", () => {
	debug("Audio capture stopped");
});

audio.on("chunk", (chunk: Buffer) => {
	const base64 = chunk.toString("base64");
	network.sendAudio(base64);
});

audio.on("error", (err: Error) => {
	debug(`Audio error: ${err.message}`);
	broadcastError("AUDIO_UNAVAILABLE", err.message, true);
});

audio.on("restarting", (attempt: number, delayMs: number) => {
	debug(`Audio restarting (attempt ${attempt}, delay ${delayMs}ms)`);
});

audio.on("failed", (err: Error) => {
	debug(`Audio failed: ${err.message}`);
	broadcastError(
		"AUDIO_UNAVAILABLE",
		err.message,
		false,
		`Audio backend (${audio.getBackendName()}) failed. Check audio device availability.`,
	);
	stateMachine.transition({ type: "FATAL_ERROR", message: err.message });
});

// ============================================================================
// Network supervisor event handlers
// ============================================================================

network.on("connected", () => {
	debug("WebSocket connected");
	stateMachine.transition({ type: "WS_READY" });
});

network.on("disconnected", () => {
	debug("WebSocket disconnected");
});

network.on("reconnecting", (attempt: number, delayMs: number) => {
	debug(`WebSocket reconnecting (attempt ${attempt}, delay ${delayMs}ms)`);
	stateMachine.transition({ type: "WS_DISCONNECTED" });
});

network.on("failed", (err: Error) => {
	debug(`WebSocket failed: ${err.message}`);
	broadcastError("NETWORK_ERROR", err.message, false);
	stateMachine.transition({ type: "FATAL_ERROR", message: err.message });
});

network.on("error", (err: Error) => {
	debug(`WebSocket error: ${err.message}`);
	// Check for auth errors - match both HTTP status codes and Realtime API error types
	if (
		err.message.includes("401") ||
		err.message.includes("Unauthorized") ||
		err.message.startsWith("[unauthorized]") ||
		err.message.startsWith("[authentication_error]") ||
		err.message.startsWith("[invalid_api_key]")
	) {
		broadcastError(
			"AUTH_FAILED",
			"Invalid or unauthorized API key",
			false,
			"Check your OPENAI_API_KEY at https://platform.openai.com/api-keys",
		);
	} else if (err.message.includes("timed out")) {
		// Connection timeout - could be network or auth issue
		broadcastError(
			"NETWORK_ERROR",
			err.message,
			true,
			"Check your network connection and OPENAI_API_KEY",
		);
	} else {
		broadcastError("NETWORK_ERROR", err.message, true);
	}
});

network.on("speech_started", (itemId: string) => {
	debug(`Speech started: ${itemId}`);
	itemTexts.set(itemId, "");
	sendToOwner({ type: "speech_started", item_id: itemId });
});

network.on("speech_stopped", (itemId: string) => {
	debug(`Speech stopped: ${itemId}`);
	sendToOwner({ type: "speech_stopped", item_id: itemId });
});

network.on("delta", (itemId: string, delta: string) => {
	// Accumulate text for this item
	const current = itemTexts.get(itemId) ?? "";
	const newText = current + delta;
	itemTexts.set(itemId, newText);
	// Emit accumulated text (not just delta) for easier UI rendering
	sendToOwner({
		type: "partial_transcript",
		item_id: itemId,
		text: newText,
	});
});

network.on("completed", (itemId: string, transcript: string) => {
	debug(`Transcription completed: ${itemId}`);
	itemTexts.delete(itemId);
	sendToOwner({
		type: "final_transcript",
		item_id: itemId,
		text: transcript,
	});

	// If we're in flushing state, this final transcript completes the flush
	if (stateMachine.getState() === "flushing") {
		stateMachine.transition({ type: "FINAL_TRANSCRIPT_RECEIVED" });
	}
});

// ============================================================================
// Socket server event handlers
// ============================================================================

server.on("client_connected", (clientId: string) => {
	debug(`Client connected: ${clientId}`);
	idleExitPolicy.onClientConnect();
	// Send current status to the new client
	server.send(clientId, {
		type: "status",
		state: stateMachine.getState(),
		audio_ok: audio.isRunning(),
		ws_ok: network.isConnected(),
	});
});

server.on("client_disconnected", (clientId: string) => {
	debug(`Client disconnected: ${clientId}`);
	idleExitPolicy.onClientDisconnect();

	// If the disconnected client was the session owner, force-stop the session
	if (sessionOwner === clientId) {
		debug(`Session owner disconnected, stopping session`);
		// Note: sessionOwner is cleared by the transition callback when state becomes idle

		// Force stop if we're in an active state
		const state = stateMachine.getState();
		if (state !== "idle" && state !== "error") {
			audio.stop();
			network.disconnect();
			itemTexts.clear();
			// Use RESET to force-return to idle from any state
			stateMachine.transition({ type: "RESET" });
		}
	}
});

server.on("client_message", (clientId: string, msg) => {
	debug(`Message from ${clientId}: ${JSON.stringify(msg)}`);

	switch (msg.type) {
		case "initialize":
			// Already handled by server.ts (sends 'initialized' response)
			break;

		case "start_listening":
			handleStartListening(clientId);
			break;

		case "stop_listening":
			handleStopListening(clientId);
			break;

		case "set_mode":
			// Future: handle mode switching
			debug(`Mode switch requested: ${msg.mode}`);
			break;

		case "disconnect":
			// Client is disconnecting gracefully - nothing to do
			break;
	}
});

server.on("error", (err: Error) => {
	debug(`Server error: ${err.message}`);
});

// ============================================================================
// Command handlers
// ============================================================================

function handleStartListening(clientId: string): void {
	const state = stateMachine.getState();

	// Check session ownership
	if (sessionOwner !== null && sessionOwner !== clientId) {
		debug(
			`Session busy: ${sessionOwner} owns the session, rejecting ${clientId}`,
		);
		sendErrorToClient(
			clientId,
			"SESSION_BUSY",
			"Another client is already dictating",
			true,
			"Wait for the other session to end",
		);
		return;
	}

	// Idempotent: if this client already owns the session and we're listening
	if (sessionOwner === clientId && state === "listening") {
		debug("Already listening (same owner)");
		return;
	}

	if (state === "error") {
		debug("Cannot start from error state - reset first");
		return;
	}

	// Transition to audio_starting and begin the startup sequence
	const started = stateMachine.transition({ type: "START_LISTENING" });
	if (!started) {
		debug(`Cannot start listening from state: ${state}`);
		return;
	}

	// Claim ownership
	sessionOwner = clientId;
	debug(`Session owner set: ${clientId}`);

	// Start both supervisors - state machine handles the transitions
	network.connect();
	audio.start();
}

function handleStopListening(clientId: string): void {
	const state = stateMachine.getState();

	// Only the session owner can stop listening
	if (sessionOwner !== null && sessionOwner !== clientId) {
		debug(`Stop rejected: ${clientId} is not the owner (${sessionOwner})`);
		return;
	}

	if (state === "idle") {
		debug("Already idle");
		return;
	}

	// Transition to flushing (waiting for final transcripts)
	// Note: ownership is NOT cleared here - it persists through flushing
	// and is only cleared when the state machine transitions to idle
	stateMachine.transition({ type: "STOP_LISTENING" });

	// Stop audio capture
	audio.stop();

	// Don't disconnect WebSocket immediately - wait for final transcripts
	// The state machine will transition to idle when FINAL_TRANSCRIPT_RECEIVED
	// For now, if we're not mid-transcription, just go straight to idle
	if (itemTexts.size === 0) {
		stateMachine.transition({ type: "FINAL_TRANSCRIPT_RECEIVED" });
		network.disconnect();
	} else {
		// Set a timeout to force disconnect if no final transcript arrives
		setTimeout(() => {
			if (stateMachine.getState() === "flushing") {
				debug("Flush timeout - forcing disconnect");
				stateMachine.transition({ type: "FINAL_TRANSCRIPT_RECEIVED" });
				network.disconnect();
				itemTexts.clear();
			}
		}, 5000);
	}
}

// ============================================================================
// Graceful shutdown
// ============================================================================

function shutdown(): void {
	debug("Shutting down...");
	idleExitPolicy.cancel();
	audio.stop();
	network.disconnect();
	server.close();
	process.exit(0);
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

// ============================================================================
// Start server
// ============================================================================

server.listen();
debug(`Daemon v${DAEMON_VERSION} ready, listening on socket`);
