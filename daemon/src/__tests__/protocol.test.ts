import { describe, it, expect } from 'vitest';
import {
  ClientMessageSchema,
  DaemonMessageSchema,
} from '../protocol.js';

describe('ClientMessageSchema', () => {
  it('validates start command', () => {
    const result = ClientMessageSchema.safeParse({ type: 'start' });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe('start');
    }
  });

  it('validates stop command', () => {
    const result = ClientMessageSchema.safeParse({ type: 'stop' });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe('stop');
    }
  });

  it('validates config command with options', () => {
    const result = ClientMessageSchema.safeParse({
      type: 'config',
      model: 'gpt-4o-transcribe',
      prompt: 'technical terms',
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe('config');
      if (result.data.type === 'config') {
        expect(result.data.model).toBe('gpt-4o-transcribe');
        expect(result.data.prompt).toBe('technical terms');
      }
    }
  });

  it('validates config command without options', () => {
    const result = ClientMessageSchema.safeParse({ type: 'config' });
    expect(result.success).toBe(true);
  });

  it('rejects invalid command type', () => {
    const result = ClientMessageSchema.safeParse({ type: 'invalid' });
    expect(result.success).toBe(false);
  });

  it('rejects missing type', () => {
    const result = ClientMessageSchema.safeParse({});
    expect(result.success).toBe(false);
  });
});

describe('DaemonMessageSchema', () => {
  it('validates status message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'status',
      state: 'ready',
      message: 'Connected',
    });
    expect(result.success).toBe(true);
  });

  it('validates status message without optional message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'status',
      state: 'recording',
    });
    expect(result.success).toBe(true);
  });

  it('validates all status states', () => {
    const states = ['connecting', 'ready', 'recording', 'stopped', 'error'];
    for (const state of states) {
      const result = DaemonMessageSchema.safeParse({ type: 'status', state });
      expect(result.success).toBe(true);
    }
  });

  it('rejects invalid status state', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'status',
      state: 'invalid_state',
    });
    expect(result.success).toBe(false);
  });

  it('validates delta message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'delta',
      item_id: 'item_123',
      text: 'hello world',
    });
    expect(result.success).toBe(true);
  });

  it('validates final message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'final',
      item_id: 'item_123',
      text: 'Hello, world!',
    });
    expect(result.success).toBe(true);
  });

  it('validates speech_started message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'speech_started',
      item_id: 'item_456',
    });
    expect(result.success).toBe(true);
  });

  it('validates speech_stopped message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'speech_stopped',
      item_id: 'item_456',
    });
    expect(result.success).toBe(true);
  });

  it('validates error message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'error',
      code: 'AUDIO_ERROR',
      message: 'Failed to start audio capture',
    });
    expect(result.success).toBe(true);
  });

  it('validates debug message', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'debug',
      message: 'WebSocket connected',
    });
    expect(result.success).toBe(true);
  });

  it('rejects invalid message type', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'unknown',
      data: 'test',
    });
    expect(result.success).toBe(false);
  });
});
