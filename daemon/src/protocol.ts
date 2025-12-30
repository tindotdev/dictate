import { z } from 'zod';

// ============================================================================
// Client → Daemon messages (received on stdin)
// ============================================================================

export const ClientMessageSchema = z.discriminatedUnion('type', [
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
// Daemon → Client messages (emitted on stdout)
// ============================================================================

export const StatusStateSchema = z.enum([
  'connecting',
  'ready',
  'recording',
  'stopped',
  'error',
]);

export type StatusState = z.infer<typeof StatusStateSchema>;

export const DaemonMessageSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('status'),
    state: StatusStateSchema,
    message: z.string().optional(),
  }),
  z.object({
    type: z.literal('delta'),
    item_id: z.string(),
    text: z.string(),
  }),
  z.object({
    type: z.literal('final'),
    item_id: z.string(),
    text: z.string(),
  }),
  z.object({
    type: z.literal('speech_started'),
    item_id: z.string(),
  }),
  z.object({
    type: z.literal('speech_stopped'),
    item_id: z.string(),
  }),
  z.object({
    type: z.literal('error'),
    code: z.string(),
    message: z.string(),
  }),
  z.object({
    type: z.literal('debug'),
    message: z.string(),
  }),
]);

export type DaemonMessage = z.infer<typeof DaemonMessageSchema>;

// ============================================================================
// Emit helpers
// ============================================================================

export function emit(msg: DaemonMessage): void {
  const line = JSON.stringify(msg);
  process.stdout.write(line + '\n');
}

export function emitStatus(state: StatusState, message?: string): void {
  emit({ type: 'status', state, message });
}

export function emitError(code: string, message: string): void {
  emit({ type: 'error', code, message });
}

export function emitDebug(message: string): void {
  if (process.env.DEBUG === '1') {
    emit({ type: 'debug', message });
  }
}

export function emitDelta(itemId: string, text: string): void {
  emit({ type: 'delta', item_id: itemId, text });
}

export function emitFinal(itemId: string, text: string): void {
  emit({ type: 'final', item_id: itemId, text });
}

export function emitSpeechStarted(itemId: string): void {
  emit({ type: 'speech_started', item_id: itemId });
}

export function emitSpeechStopped(itemId: string): void {
  emit({ type: 'speech_stopped', item_id: itemId });
}
