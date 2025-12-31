import { z } from 'zod';

// ============================================================================
// Daemon State Machine
// ============================================================================

export const DaemonStateSchema = z.enum([
  'idle',           // Not listening, ready to start
  'audio_starting', // pw-cat spawning
  'listening',      // Actively capturing + streaming
  'flushing',       // Stopped capture, waiting for final transcript
  'reconnecting',   // WebSocket down, retrying
  'error',          // Fatal error (bad config, etc.)
]);

export type DaemonState = z.infer<typeof DaemonStateSchema>;

// ============================================================================
// Error Codes
// ============================================================================

export const ErrorCodeSchema = z.enum([
  // sayctl errors
  'DAEMON_UNAVAILABLE', // Socket doesn't exist (systemd not set up)

  // Daemon errors
  'AUTH_FAILED',        // Invalid API key
  'CONFIG_ERROR',       // Missing/invalid config
  'AUDIO_UNAVAILABLE',  // pw-cat can't access mic
  'NETWORK_ERROR',      // WebSocket issues
  'RATE_LIMITED',       // API quota exceeded
  'INTERNAL_ERROR',     // Unexpected daemon error
]);

export type ErrorCode = z.infer<typeof ErrorCodeSchema>;

// ============================================================================
// Client → Daemon messages
// ============================================================================

export const ClientMessageSchema = z.discriminatedUnion('type', [
  // Initial handshake
  z.object({
    type: z.literal('initialize'),
    client_id: z.string().optional(),
    version: z.string(),
  }),

  // Audio control
  z.object({ type: z.literal('start_listening') }),
  z.object({ type: z.literal('stop_listening') }),

  // Mode switching (future)
  z.object({
    type: z.literal('set_mode'),
    mode: z.enum(['dictation', 'command']),
  }),

  // Graceful disconnect
  z.object({ type: z.literal('disconnect') }),

  // DEPRECATED: Legacy message types (remove after main.ts refactor)
  z.object({ type: z.literal('start') }),
  z.object({ type: z.literal('stop') }),
  z.object({
    type: z.literal('config'),
    model: z.string().optional(),
    prompt: z.string().optional(),
  }),
]);

export type ClientMessage = z.infer<typeof ClientMessageSchema>;

// ============================================================================
// Daemon → Client messages
// ============================================================================

export const DaemonMessageSchema = z.discriminatedUnion('type', [
  // Handshake response
  z.object({
    type: z.literal('initialized'),
    client_id: z.string(),
    daemon_version: z.string(),
  }),

  // Status updates (state machine transitions)
  z.object({
    type: z.literal('status'),
    state: DaemonStateSchema,
    audio_ok: z.boolean(),
    ws_ok: z.boolean(),
    message: z.string().optional(),
  }),

  // Transcription events
  z.object({
    type: z.literal('speech_started'),
    item_id: z.string(),
  }),
  z.object({
    type: z.literal('speech_stopped'),
    item_id: z.string(),
  }),
  z.object({
    type: z.literal('partial_transcript'),
    item_id: z.string(),
    text: z.string(),
  }),
  z.object({
    type: z.literal('final_transcript'),
    item_id: z.string(),
    text: z.string(),
  }),

  // Errors (typed + actionable)
  z.object({
    type: z.literal('error'),
    code: ErrorCodeSchema,
    message: z.string(),
    recoverable: z.boolean(),
    hint: z.string().optional(),
  }),

  // Debug messages
  z.object({
    type: z.literal('debug'),
    message: z.string(),
  }),
]);

export type DaemonMessage = z.infer<typeof DaemonMessageSchema>;

// ============================================================================
// sayctl-specific status events (emitted before daemon connection)
// These use a simpler status format without audio_ok/ws_ok
// ============================================================================

export const SayctlStateSchema = z.enum([
  'connecting',     // Initial connection attempt
  'connected',      // Socket connected, forwarding
  'reconnecting',   // Lost connection, retrying
]);

export type SayctlState = z.infer<typeof SayctlStateSchema>;

export const SayctlMessageSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('status'),
    state: SayctlStateSchema,
  }),
  z.object({
    type: z.literal('error'),
    code: z.literal('DAEMON_UNAVAILABLE'),
    message: z.string(),
    recoverable: z.literal(false),
    hint: z.string(),
  }),
]);

export type SayctlMessage = z.infer<typeof SayctlMessageSchema>;

// ============================================================================
// Protocol version
// ============================================================================

export const PROTOCOL_VERSION = '2.0.0';
export const DAEMON_VERSION = '0.2.0';

// ============================================================================
// Emit helpers (for daemon stdout - will be refactored for socket)
// ============================================================================

export function emit(msg: DaemonMessage): void {
  const line = JSON.stringify(msg);
  process.stdout.write(line + '\n');
}

export function emitStatus(
  state: DaemonState,
  audioOk: boolean,
  wsOk: boolean,
  message?: string
): void {
  emit({ type: 'status', state, audio_ok: audioOk, ws_ok: wsOk, message });
}

export function emitError(
  code: ErrorCode,
  message: string,
  recoverable: boolean,
  hint?: string
): void {
  emit({ type: 'error', code, message, recoverable, hint });
}

export function emitDebug(message: string): void {
  if (process.env.DEBUG === '1') {
    emit({ type: 'debug', message });
  }
}

export function emitPartialTranscript(itemId: string, text: string): void {
  emit({ type: 'partial_transcript', item_id: itemId, text });
}

export function emitFinalTranscript(itemId: string, text: string): void {
  emit({ type: 'final_transcript', item_id: itemId, text });
}

export function emitSpeechStarted(itemId: string): void {
  emit({ type: 'speech_started', item_id: itemId });
}

export function emitSpeechStopped(itemId: string): void {
  emit({ type: 'speech_stopped', item_id: itemId });
}

export function emitInitialized(clientId: string): void {
  emit({ type: 'initialized', client_id: clientId, daemon_version: DAEMON_VERSION });
}

// ============================================================================
// Backwards-compatible shims (DEPRECATED - remove after main.ts refactor)
// ============================================================================

/** @deprecated Use emitPartialTranscript */
export function emitDelta(itemId: string, text: string): void {
  emitPartialTranscript(itemId, text);
}

/** @deprecated Use emitFinalTranscript */
export function emitFinal(itemId: string, text: string): void {
  emitFinalTranscript(itemId, text);
}

// Old status emit for backwards compat (uses 'stopped' instead of 'idle', etc.)
type OldStatusState = 'connecting' | 'ready' | 'recording' | 'stopped' | 'error';

/** @deprecated Will be removed after main.ts refactor */
export function emitStatusCompat(state: OldStatusState, message?: string): void {
  // Map old states to new states
  const stateMap: Record<OldStatusState, DaemonState> = {
    connecting: 'audio_starting',
    ready: 'idle',
    recording: 'listening',
    stopped: 'idle',
    error: 'error',
  };
  const newState = stateMap[state] ?? 'idle';
  emit({ type: 'status', state: newState, audio_ok: true, ws_ok: true, message });
}
