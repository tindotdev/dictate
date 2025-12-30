import * as readline from 'readline';
import * as path from 'path';
import { loadConfig } from './config.js';
import {
  ClientMessageSchema,
  emitStatus,
  emitError,
  emitDelta,
  emitFinal,
  emitSpeechStarted,
  emitSpeechStopped,
  emitDebug,
} from './protocol.js';
import { AudioCapture } from './pipewire.js';
import { RealtimeClient } from './realtime.js';

// Load .env file from daemon directory synchronously
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

// Load configuration
let config: ReturnType<typeof loadConfig>;
try {
  config = loadConfig();
} catch (err) {
  emitError('CONFIG_ERROR', (err as Error).message);
  process.exit(1);
}

// Initialize components
const audio = new AudioCapture();
const realtime = new RealtimeClient(config);

// Track accumulated text per item_id (for delta -> full text conversion)
const itemTexts = new Map<string, string>();

// ============================================================================
// Wire audio capture -> WebSocket
// ============================================================================

audio.on('chunk', (chunk: Buffer) => {
  const base64 = chunk.toString('base64');
  realtime.sendAudio(base64);
});

audio.on('error', (err) => {
  emitError('AUDIO_ERROR', err.message);
  emitStatus('error', err.message);
});

audio.on('close', (code) => {
  if (code !== 0 && code !== null) {
    emitError('AUDIO_CLOSED', `pw-cat exited with code ${code}`);
  }
});

// ============================================================================
// Wire WebSocket events -> JSONL stdout
// ============================================================================

realtime.on('open', () => {
  emitStatus('ready');
  // Start audio capture after connection is ready
  if (!audio.isRunning()) {
    audio.start();
    emitStatus('recording');
  }
});

realtime.on('speech_started', (itemId) => {
  itemTexts.set(itemId, '');
  emitSpeechStarted(itemId);
});

realtime.on('speech_stopped', (itemId) => {
  emitSpeechStopped(itemId);
});

realtime.on('delta', (itemId, delta) => {
  // Accumulate text for this item
  const current = itemTexts.get(itemId) ?? '';
  const newText = current + delta;
  itemTexts.set(itemId, newText);
  // Emit accumulated text (not just delta) for easier UI rendering
  emitDelta(itemId, newText);
});

realtime.on('completed', (itemId, transcript) => {
  itemTexts.delete(itemId);
  emitFinal(itemId, transcript);
});

realtime.on('error', (err) => {
  emitError('REALTIME_ERROR', err.message);
});

realtime.on('close', (code, reason) => {
  audio.stop();
  emitStatus('stopped', `WebSocket closed: ${code} ${reason}`);
});

// ============================================================================
// Read commands from stdin (JSONL)
// ============================================================================

const rl = readline.createInterface({
  input: process.stdin,
  terminal: false,
});

rl.on('line', (line) => {
  if (!line.trim()) return;

  try {
    const parsed = JSON.parse(line);
    const msg = ClientMessageSchema.parse(parsed);

    switch (msg.type) {
      case 'start':
        emitDebug('Received start command');
        if (!realtime.isConnected()) {
          emitStatus('connecting');
          realtime.connect();
          // Audio starts after 'open' event
        } else if (!audio.isRunning()) {
          audio.start();
          emitStatus('recording');
        }
        break;

      case 'stop':
        emitDebug('Received stop command');
        audio.stop();
        realtime.disconnect();
        itemTexts.clear();
        emitStatus('stopped');
        break;

      case 'config':
        emitDebug(`Received config update: ${JSON.stringify(msg)}`);
        // Runtime config updates could be implemented here
        // For now, config is loaded at startup only
        break;
    }
  } catch (err) {
    emitError('PARSE_ERROR', (err as Error).message);
  }
});

rl.on('close', () => {
  emitDebug('stdin closed, shutting down');
  audio.stop();
  realtime.disconnect();
  process.exit(0);
});

// ============================================================================
// Graceful shutdown
// ============================================================================

function shutdown() {
  emitDebug('Shutting down...');
  audio.stop();
  realtime.disconnect();
  process.exit(0);
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

// ============================================================================
// Initial state
// ============================================================================

emitStatus('stopped');
emitDebug('Daemon ready, waiting for commands');
