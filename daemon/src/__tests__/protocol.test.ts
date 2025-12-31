import { describe, it, expect } from 'bun:test';
import {
  ClientMessageSchema,
  DaemonMessageSchema,
  DaemonStateSchema,
  ErrorCodeSchema,
  DictatectlMessageSchema,
} from '../protocol.js';

describe('ClientMessageSchema', () => {
  describe('new protocol messages', () => {
    it('validates initialize command', () => {
      const result = ClientMessageSchema.safeParse({
        type: 'initialize',
        version: '2.0.0',
      });
      expect(result.success).toBe(true);
    });

    it('validates initialize with client_id', () => {
      const result = ClientMessageSchema.safeParse({
        type: 'initialize',
        client_id: 'nvim-123',
        version: '2.0.0',
      });
      expect(result.success).toBe(true);
    });

    it('validates start_listening command', () => {
      const result = ClientMessageSchema.safeParse({ type: 'start_listening' });
      expect(result.success).toBe(true);
    });

    it('validates stop_listening command', () => {
      const result = ClientMessageSchema.safeParse({ type: 'stop_listening' });
      expect(result.success).toBe(true);
    });

    it('validates set_mode command', () => {
      const result = ClientMessageSchema.safeParse({
        type: 'set_mode',
        mode: 'dictation',
      });
      expect(result.success).toBe(true);
    });

    it('validates disconnect command', () => {
      const result = ClientMessageSchema.safeParse({ type: 'disconnect' });
      expect(result.success).toBe(true);
    });
  });

  describe('legacy protocol messages (deprecated)', () => {
    it('validates start command', () => {
      const result = ClientMessageSchema.safeParse({ type: 'start' });
      expect(result.success).toBe(true);
    });

    it('validates stop command', () => {
      const result = ClientMessageSchema.safeParse({ type: 'stop' });
      expect(result.success).toBe(true);
    });

    it('validates config command with options', () => {
      const result = ClientMessageSchema.safeParse({
        type: 'config',
        model: 'gpt-4o-transcribe',
        prompt: 'technical terms',
      });
      expect(result.success).toBe(true);
    });

    it('validates config command without options', () => {
      const result = ClientMessageSchema.safeParse({ type: 'config' });
      expect(result.success).toBe(true);
    });
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
  describe('initialized message', () => {
    it('validates initialized response', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'initialized',
        client_id: 'client-123',
        daemon_version: '0.2.0',
      });
      expect(result.success).toBe(true);
    });
  });

  describe('status message', () => {
    it('validates status message with all fields', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'status',
        state: 'listening',
        audio_ok: true,
        ws_ok: true,
        message: 'Connected',
      });
      expect(result.success).toBe(true);
    });

    it('validates status message without optional message', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'status',
        state: 'idle',
        audio_ok: false,
        ws_ok: false,
      });
      expect(result.success).toBe(true);
    });

    it('validates all daemon states', () => {
      const states = ['idle', 'audio_starting', 'listening', 'flushing', 'reconnecting', 'error'];
      for (const state of states) {
        const result = DaemonMessageSchema.safeParse({
          type: 'status',
          state,
          audio_ok: true,
          ws_ok: true,
        });
        expect(result.success).toBe(true);
      }
    });

    it('rejects invalid status state', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'status',
        state: 'invalid_state',
        audio_ok: true,
        ws_ok: true,
      });
      expect(result.success).toBe(false);
    });

    it('rejects status without audio_ok', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'status',
        state: 'idle',
        ws_ok: true,
      });
      expect(result.success).toBe(false);
    });
  });

  describe('transcription messages', () => {
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

    it('validates partial_transcript message', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'partial_transcript',
        item_id: 'item_123',
        text: 'hello world',
      });
      expect(result.success).toBe(true);
    });

    it('validates final_transcript message', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'final_transcript',
        item_id: 'item_123',
        text: 'Hello, world!',
      });
      expect(result.success).toBe(true);
    });
  });

  describe('error message', () => {
    it('validates error message with all fields', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'error',
        code: 'AUDIO_UNAVAILABLE',
        message: 'Failed to start audio capture',
        recoverable: true,
        hint: 'Check if pw-cat is installed',
      });
      expect(result.success).toBe(true);
    });

    it('validates error message without hint', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'error',
        code: 'AUTH_FAILED',
        message: 'Invalid API key',
        recoverable: false,
      });
      expect(result.success).toBe(true);
    });

    it('validates all error codes', () => {
      const codes = [
        'DAEMON_UNAVAILABLE',
        'AUTH_FAILED',
        'CONFIG_ERROR',
        'AUDIO_UNAVAILABLE',
        'NETWORK_ERROR',
        'RATE_LIMITED',
        'INTERNAL_ERROR',
      ];
      for (const code of codes) {
        const result = DaemonMessageSchema.safeParse({
          type: 'error',
          code,
          message: 'test',
          recoverable: true,
        });
        expect(result.success).toBe(true);
      }
    });

    it('rejects error without recoverable flag', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'error',
        code: 'AUTH_FAILED',
        message: 'Invalid API key',
      });
      expect(result.success).toBe(false);
    });
  });

  describe('debug message', () => {
    it('validates debug message', () => {
      const result = DaemonMessageSchema.safeParse({
        type: 'debug',
        message: 'WebSocket connected',
      });
      expect(result.success).toBe(true);
    });
  });

  it('rejects invalid message type', () => {
    const result = DaemonMessageSchema.safeParse({
      type: 'unknown',
      data: 'test',
    });
    expect(result.success).toBe(false);
  });
});

describe('DictatectlMessageSchema', () => {
  it('validates dictatectl status message', () => {
    const result = DictatectlMessageSchema.safeParse({
      type: 'status',
      state: 'connecting',
    });
    expect(result.success).toBe(true);
  });

  it('validates all dictatectl states', () => {
    const states = ['connecting', 'connected', 'reconnecting'];
    for (const state of states) {
      const result = DictatectlMessageSchema.safeParse({
        type: 'status',
        state,
      });
      expect(result.success).toBe(true);
    }
  });

  it('validates dictatectl DAEMON_UNAVAILABLE error', () => {
    const result = DictatectlMessageSchema.safeParse({
      type: 'error',
      code: 'DAEMON_UNAVAILABLE',
      message: 'Cannot connect to dictate daemon',
      recoverable: false,
      hint: 'Run: systemctl --user enable --now dictate.service',
    });
    expect(result.success).toBe(true);
  });
});

describe('DaemonStateSchema', () => {
  it('accepts valid states', () => {
    const states = ['idle', 'audio_starting', 'listening', 'flushing', 'reconnecting', 'error'];
    for (const state of states) {
      const result = DaemonStateSchema.safeParse(state);
      expect(result.success).toBe(true);
    }
  });

  it('rejects invalid state', () => {
    const result = DaemonStateSchema.safeParse('invalid');
    expect(result.success).toBe(false);
  });
});

describe('ErrorCodeSchema', () => {
  it('accepts valid error codes', () => {
    const codes = [
      'DAEMON_UNAVAILABLE',
      'AUTH_FAILED',
      'CONFIG_ERROR',
      'AUDIO_UNAVAILABLE',
      'NETWORK_ERROR',
      'RATE_LIMITED',
      'INTERNAL_ERROR',
    ];
    for (const code of codes) {
      const result = ErrorCodeSchema.safeParse(code);
      expect(result.success).toBe(true);
    }
  });

  it('rejects invalid error code', () => {
    const result = ErrorCodeSchema.safeParse('INVALID_CODE');
    expect(result.success).toBe(false);
  });
});
