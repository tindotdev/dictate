import { describe, expect, it, mock, beforeEach } from 'bun:test';
import { DaemonStateMachine, createStateMachine, type StateEvent } from '../state-machine.js';

describe('DaemonStateMachine', () => {
  let sm: DaemonStateMachine;

  beforeEach(() => {
    sm = createStateMachine();
  });

  describe('initial state', () => {
    it('starts in idle state', () => {
      expect(sm.getState()).toBe('idle');
    });

    it('has audio and ws not ready', () => {
      expect(sm.isAudioReady()).toBe(false);
      expect(sm.isWsReady()).toBe(false);
    });
  });

  describe('idle -> audio_starting', () => {
    it('transitions on START_LISTENING', () => {
      const result = sm.transition({ type: 'START_LISTENING' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('audio_starting');
    });

    it('emits transition event', () => {
      const handler = mock(() => {});
      sm.on('transition', handler);

      sm.transition({ type: 'START_LISTENING' });

      expect(handler).toHaveBeenCalledWith(
        'idle',
        'audio_starting',
        { type: 'START_LISTENING' }
      );
    });
  });

  describe('audio_starting -> listening', () => {
    beforeEach(() => {
      sm.transition({ type: 'START_LISTENING' });
    });

    it('requires both audio and ws ready', () => {
      // Just audio ready - no transition
      sm.transition({ type: 'AUDIO_READY' });
      expect(sm.getState()).toBe('audio_starting');
      expect(sm.isAudioReady()).toBe(true);

      // Now ws ready - should transition
      const result = sm.transition({ type: 'WS_READY' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('listening');
    });

    it('works with ws first then audio', () => {
      sm.transition({ type: 'WS_READY' });
      expect(sm.getState()).toBe('audio_starting');
      expect(sm.isWsReady()).toBe(true);

      sm.transition({ type: 'AUDIO_READY' });
      expect(sm.getState()).toBe('listening');
    });
  });

  describe('listening -> flushing', () => {
    beforeEach(() => {
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });
    });

    it('transitions on STOP_LISTENING', () => {
      expect(sm.getState()).toBe('listening');

      const result = sm.transition({ type: 'STOP_LISTENING' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('flushing');
    });
  });

  describe('flushing -> idle', () => {
    beforeEach(() => {
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });
      sm.transition({ type: 'STOP_LISTENING' });
    });

    it('transitions on FINAL_TRANSCRIPT_RECEIVED', () => {
      expect(sm.getState()).toBe('flushing');

      const result = sm.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('idle');
    });

    it('resets audio/ws ready flags', () => {
      sm.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });

      expect(sm.isAudioReady()).toBe(false);
      expect(sm.isWsReady()).toBe(false);
    });
  });

  describe('reconnecting flow', () => {
    beforeEach(() => {
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });
    });

    it('transitions to reconnecting on WS_DISCONNECTED from listening', () => {
      expect(sm.getState()).toBe('listening');

      const result = sm.transition({ type: 'WS_DISCONNECTED' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('reconnecting');
      expect(sm.isWsReady()).toBe(false);
    });

    it('transitions to reconnecting on WS_DISCONNECTED from flushing', () => {
      sm.transition({ type: 'STOP_LISTENING' });
      expect(sm.getState()).toBe('flushing');

      const result = sm.transition({ type: 'WS_DISCONNECTED' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('reconnecting');
    });

    it('transitions back to listening on WS_RECONNECTED', () => {
      sm.transition({ type: 'WS_DISCONNECTED' });
      expect(sm.getState()).toBe('reconnecting');

      const result = sm.transition({ type: 'WS_RECONNECTED' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('listening');
      expect(sm.isWsReady()).toBe(true);
    });

    it('can also reconnect via WS_READY', () => {
      sm.transition({ type: 'WS_DISCONNECTED' });
      expect(sm.getState()).toBe('reconnecting');

      const result = sm.transition({ type: 'WS_READY' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('listening');
    });
  });

  describe('error state', () => {
    it('can transition to error from any state', () => {
      const states: Array<() => void> = [
        () => {}, // idle
        () => sm.transition({ type: 'START_LISTENING' }), // audio_starting
        () => {
          sm.transition({ type: 'START_LISTENING' });
          sm.transition({ type: 'AUDIO_READY' });
          sm.transition({ type: 'WS_READY' });
        }, // listening
      ];

      for (const setup of states) {
        const freshSm = createStateMachine();
        // Must attach error listener to prevent unhandled error throw
        freshSm.on('error', () => {});
        setup.call({ sm: freshSm });

        const result = freshSm.transition({
          type: 'FATAL_ERROR',
          message: 'test error',
        });
        expect(result).toBe(true);
        expect(freshSm.getState()).toBe('error');
      }
    });

    it('emits error event with message', () => {
      const handler = mock(() => {});
      sm.on('error', handler);

      sm.transition({ type: 'FATAL_ERROR', message: 'something went wrong' });

      expect(handler).toHaveBeenCalledWith('something went wrong');
    });

    it('resets audio/ws ready flags', () => {
      // Must attach error listener to prevent unhandled error throw
      sm.on('error', () => {});

      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });

      expect(sm.isAudioReady()).toBe(true);
      expect(sm.isWsReady()).toBe(true);

      sm.transition({ type: 'FATAL_ERROR', message: 'error' });

      expect(sm.isAudioReady()).toBe(false);
      expect(sm.isWsReady()).toBe(false);
    });
  });

  describe('reset', () => {
    it('transitions to idle from any state', () => {
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });
      expect(sm.getState()).toBe('listening');

      const result = sm.transition({ type: 'RESET' });
      expect(result).toBe(true);
      expect(sm.getState()).toBe('idle');
    });

    it('resets audio/ws flags', () => {
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });

      sm.transition({ type: 'RESET' });

      expect(sm.isAudioReady()).toBe(false);
      expect(sm.isWsReady()).toBe(false);
    });
  });

  describe('invalid transitions', () => {
    it('returns false for invalid transition', () => {
      // Can't stop listening when idle
      const result = sm.transition({ type: 'STOP_LISTENING' });
      expect(result).toBe(false);
      expect(sm.getState()).toBe('idle');
    });

    it('does not emit event for invalid transition', () => {
      const handler = mock(() => {});
      sm.on('transition', handler);

      sm.transition({ type: 'STOP_LISTENING' });

      expect(handler).not.toHaveBeenCalled();
    });

    it('cannot reconnect from idle', () => {
      const result = sm.transition({ type: 'WS_RECONNECTED' });
      expect(result).toBe(false);
      expect(sm.getState()).toBe('idle');
    });

    it('cannot go directly from idle to listening', () => {
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });

      // Still idle because we need to START_LISTENING first
      expect(sm.getState()).toBe('idle');
    });
  });

  describe('canTransition', () => {
    it('returns true for valid transitions', () => {
      expect(sm.canTransition({ type: 'START_LISTENING' })).toBe(true);
    });

    it('returns false for invalid transitions', () => {
      expect(sm.canTransition({ type: 'STOP_LISTENING' })).toBe(false);
    });

    it('does not modify actual state', () => {
      sm.canTransition({ type: 'START_LISTENING' });
      expect(sm.getState()).toBe('idle');
    });
  });

  describe('full lifecycle', () => {
    it('completes normal dictation cycle', () => {
      const transitions: StateEvent[] = [
        { type: 'START_LISTENING' },
        { type: 'AUDIO_READY' },
        { type: 'WS_READY' },
        { type: 'STOP_LISTENING' },
        { type: 'FINAL_TRANSCRIPT_RECEIVED' },
      ];

      const expectedStates: string[] = [
        'audio_starting',
        'audio_starting', // waiting for ws
        'listening',
        'flushing',
        'idle',
      ];

      transitions.forEach((event, i) => {
        sm.transition(event);
        expect(sm.getState()).toBe(expectedStates[i]);
      });
    });

    it('handles disconnect during listening and recovery', () => {
      // Setup: get to listening state
      sm.transition({ type: 'START_LISTENING' });
      sm.transition({ type: 'AUDIO_READY' });
      sm.transition({ type: 'WS_READY' });
      expect(sm.getState()).toBe('listening');

      // Disconnect
      sm.transition({ type: 'WS_DISCONNECTED' });
      expect(sm.getState()).toBe('reconnecting');

      // Reconnect
      sm.transition({ type: 'WS_RECONNECTED' });
      expect(sm.getState()).toBe('listening');

      // Continue normal flow
      sm.transition({ type: 'STOP_LISTENING' });
      sm.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
      expect(sm.getState()).toBe('idle');
    });
  });
});
