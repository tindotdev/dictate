import * as path from 'path';
import { loadConfig, type Config } from './config.js';
import {
  type DaemonMessage,
  type DaemonState,
  type ErrorCode,
  DAEMON_VERSION,
} from './protocol.js';
import { createSocketServer, type SocketServer } from './server.js';
import { createAudioSupervisor, type AudioSupervisor } from './supervisors/audio.js';
import { createNetworkSupervisor, type NetworkSupervisor } from './supervisors/network.js';
import { createStateMachine, type DaemonStateMachine } from './state-machine.js';

// ============================================================================
// Load environment variables
// ============================================================================

const daemonDir = path.dirname(Bun.main);
const envPath = path.join(daemonDir, '..', '.env');
try {
  const envFile = Bun.file(envPath);
  if (await envFile.exists()) {
    const content = await envFile.text();
    for (const line of content.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('#')) continue;
      const eqIndex = trimmed.indexOf('=');
      if (eqIndex === -1) continue;
      const key = trimmed.slice(0, eqIndex).trim();
      const value = trimmed.slice(eqIndex + 1).trim().replace(/^["']|["']$/g, '');
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

const DEBUG = process.env.DEBUG === '1';

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

// ============================================================================
// Helper: Broadcast status to all clients
// ============================================================================

function broadcastStatus(message?: string): void {
  const state = stateMachine.getState();
  const audioOk = audio.isRunning();
  const wsOk = network.isConnected();

  server.broadcast({
    type: 'status',
    state,
    audio_ok: audioOk,
    ws_ok: wsOk,
    message,
  });
}

function broadcastError(code: ErrorCode, message: string, recoverable: boolean, hint?: string): void {
  server.broadcast({
    type: 'error',
    code,
    message,
    recoverable,
    hint,
  });
}

function broadcastMessage(msg: DaemonMessage): void {
  server.broadcast(msg);
}

// ============================================================================
// State machine event handlers
// ============================================================================

stateMachine.on('transition', (from: DaemonState, to: DaemonState) => {
  debug(`State: ${from} -> ${to}`);
  broadcastStatus();
});

stateMachine.on('error', (message: string) => {
  debug(`State machine error: ${message}`);
  broadcastError('INTERNAL_ERROR', message, false);
});

// ============================================================================
// Audio supervisor event handlers
// ============================================================================

audio.on('started', () => {
  debug('Audio capture started');
  stateMachine.transition({ type: 'AUDIO_READY' });
});

audio.on('stopped', () => {
  debug('Audio capture stopped');
});

audio.on('chunk', (chunk: Buffer) => {
  const base64 = chunk.toString('base64');
  network.sendAudio(base64);
});

audio.on('error', (err: Error) => {
  debug(`Audio error: ${err.message}`);
  broadcastError('AUDIO_UNAVAILABLE', err.message, true);
});

audio.on('restarting', (attempt: number, delayMs: number) => {
  debug(`Audio restarting (attempt ${attempt}, delay ${delayMs}ms)`);
});

audio.on('failed', (err: Error) => {
  debug(`Audio failed: ${err.message}`);
  broadcastError('AUDIO_UNAVAILABLE', err.message, false, 'Check PipeWire is running: pw-cli info');
  stateMachine.transition({ type: 'FATAL_ERROR', message: err.message });
});

// ============================================================================
// Network supervisor event handlers
// ============================================================================

network.on('connected', () => {
  debug('WebSocket connected');
  stateMachine.transition({ type: 'WS_READY' });
});

network.on('disconnected', () => {
  debug('WebSocket disconnected');
});

network.on('reconnecting', (attempt: number, delayMs: number) => {
  debug(`WebSocket reconnecting (attempt ${attempt}, delay ${delayMs}ms)`);
  stateMachine.transition({ type: 'WS_DISCONNECTED' });
});

network.on('failed', (err: Error) => {
  debug(`WebSocket failed: ${err.message}`);
  broadcastError('NETWORK_ERROR', err.message, false);
  stateMachine.transition({ type: 'FATAL_ERROR', message: err.message });
});

network.on('error', (err: Error) => {
  debug(`WebSocket error: ${err.message}`);
  // Check for auth errors
  if (err.message.includes('401') || err.message.includes('Unauthorized')) {
    broadcastError('AUTH_FAILED', 'Invalid API key', false, 'Check your OPENAI_API_KEY');
  } else {
    broadcastError('NETWORK_ERROR', err.message, true);
  }
});

network.on('speech_started', (itemId: string) => {
  debug(`Speech started: ${itemId}`);
  itemTexts.set(itemId, '');
  broadcastMessage({ type: 'speech_started', item_id: itemId });
});

network.on('speech_stopped', (itemId: string) => {
  debug(`Speech stopped: ${itemId}`);
  broadcastMessage({ type: 'speech_stopped', item_id: itemId });
});

network.on('delta', (itemId: string, delta: string) => {
  // Accumulate text for this item
  const current = itemTexts.get(itemId) ?? '';
  const newText = current + delta;
  itemTexts.set(itemId, newText);
  // Emit accumulated text (not just delta) for easier UI rendering
  broadcastMessage({ type: 'partial_transcript', item_id: itemId, text: newText });
});

network.on('completed', (itemId: string, transcript: string) => {
  debug(`Transcription completed: ${itemId}`);
  itemTexts.delete(itemId);
  broadcastMessage({ type: 'final_transcript', item_id: itemId, text: transcript });

  // If we're in flushing state, this final transcript completes the flush
  if (stateMachine.getState() === 'flushing') {
    stateMachine.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
  }
});

// ============================================================================
// Socket server event handlers
// ============================================================================

server.on('client_connected', (clientId: string) => {
  debug(`Client connected: ${clientId}`);
  // Send current status to the new client
  server.send(clientId, {
    type: 'status',
    state: stateMachine.getState(),
    audio_ok: audio.isRunning(),
    ws_ok: network.isConnected(),
  });
});

server.on('client_disconnected', (clientId: string) => {
  debug(`Client disconnected: ${clientId}`);
});

server.on('client_message', (clientId: string, msg) => {
  debug(`Message from ${clientId}: ${JSON.stringify(msg)}`);

  switch (msg.type) {
    case 'initialize':
      // Already handled by server.ts (sends 'initialized' response)
      break;

    case 'start_listening':
      handleStartListening();
      break;

    case 'stop_listening':
      handleStopListening();
      break;

    case 'set_mode':
      // Future: handle mode switching
      debug(`Mode switch requested: ${msg.mode}`);
      break;

    case 'disconnect':
      // Client is disconnecting gracefully - nothing to do
      break;
  }
});

server.on('error', (err: Error) => {
  debug(`Server error: ${err.message}`);
});

// ============================================================================
// Command handlers
// ============================================================================

function handleStartListening(): void {
  const state = stateMachine.getState();

  if (state === 'listening') {
    debug('Already listening');
    return;
  }

  if (state === 'error') {
    debug('Cannot start from error state - reset first');
    return;
  }

  // Transition to audio_starting and begin the startup sequence
  const started = stateMachine.transition({ type: 'START_LISTENING' });
  if (!started) {
    debug(`Cannot start listening from state: ${state}`);
    return;
  }

  // Start both supervisors - state machine handles the transitions
  network.connect();
  audio.start();
}

function handleStopListening(): void {
  const state = stateMachine.getState();

  if (state === 'idle') {
    debug('Already idle');
    return;
  }

  // Transition to flushing (waiting for final transcripts)
  stateMachine.transition({ type: 'STOP_LISTENING' });

  // Stop audio capture
  audio.stop();

  // Don't disconnect WebSocket immediately - wait for final transcripts
  // The state machine will transition to idle when FINAL_TRANSCRIPT_RECEIVED
  // For now, if we're not mid-transcription, just go straight to idle
  if (itemTexts.size === 0) {
    stateMachine.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
    network.disconnect();
  } else {
    // Set a timeout to force disconnect if no final transcript arrives
    setTimeout(() => {
      if (stateMachine.getState() === 'flushing') {
        debug('Flush timeout - forcing disconnect');
        stateMachine.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
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
  debug('Shutting down...');
  audio.stop();
  network.disconnect();
  server.close();
  process.exit(0);
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

// ============================================================================
// Start server
// ============================================================================

server.listen();
debug(`Daemon v${DAEMON_VERSION} ready, listening on socket`);
