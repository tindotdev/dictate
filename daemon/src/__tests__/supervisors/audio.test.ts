import { describe, expect, it, mock, afterEach } from 'bun:test';
import {
  AudioSupervisor,
  createAudioSupervisor,
  AUDIO_CONSTANTS,
  type AudioSupervisorState,
} from '../../supervisors/audio.js';

describe('AudioSupervisor', () => {
  describe('initial state', () => {
    it('starts in stopped state', () => {
      const supervisor = createAudioSupervisor();
      expect(supervisor.getState()).toBe('stopped');
      expect(supervisor.isRunning()).toBe(false);
    });
  });

  describe('AUDIO_CONSTANTS', () => {
    it('exports correct audio constants', () => {
      expect(AUDIO_CONSTANTS.SAMPLE_RATE).toBe(24000);
      expect(AUDIO_CONSTANTS.CHANNELS).toBe(1);
      expect(AUDIO_CONSTANTS.BYTES_PER_SAMPLE).toBe(2);
      expect(AUDIO_CONSTANTS.FRAME_MS).toBe(20);
      expect(AUDIO_CONSTANTS.FRAME_BYTES).toBe(960);
    });
  });

  describe('start/stop lifecycle', () => {
    let supervisor: AudioSupervisor;

    afterEach(() => {
      supervisor?.stop();
    });

    it('transitions to starting on start', () => {
      // Use /bin/true which exits immediately with success
      supervisor = createAudioSupervisor({
        pwCatPath: '/bin/true',
        backoff: { maxRetries: 0 },
      });

      const states: AudioSupervisorState[] = [];
      supervisor.on('state_change', (_from, to) => states.push(to));

      supervisor.start();

      expect(states).toContain('starting');
    });

    it('stop transitions to stopped regardless of current state', () => {
      supervisor = createAudioSupervisor({
        pwCatPath: '/bin/true',
        backoff: { maxRetries: 0 },
      });

      supervisor.start();
      supervisor.stop();

      expect(supervisor.getState()).toBe('stopped');
    });

    it('emits stopped event on stop', () => {
      supervisor = createAudioSupervisor({
        pwCatPath: '/bin/true',
        backoff: { maxRetries: 0 },
      });

      const handler = mock(() => {});
      supervisor.on('stopped', handler);

      supervisor.start();
      supervisor.stop();

      expect(handler).toHaveBeenCalled();
    });

    it('multiple starts are no-op when already starting', () => {
      supervisor = createAudioSupervisor({
        pwCatPath: '/bin/true',
        backoff: { maxRetries: 0 },
      });

      let startingCount = 0;
      supervisor.on('state_change', (_from, to) => {
        if (to === 'starting') startingCount++;
      });

      supervisor.start();
      supervisor.start();
      supervisor.start();

      expect(startingCount).toBe(1);
    });
  });

  describe('restart behavior', () => {
    it('emits restarting event when process exits unexpectedly', async () => {
      // /bin/false exits immediately with code 1 (failure)
      const supervisor = createAudioSupervisor({
        pwCatPath: '/bin/false',
        backoff: {
          baseDelayMs: 10,
          maxRetries: 3,
          jitterFactor: 0,
        },
      });

      const restartingHandler = mock((_attempt: number, _delay: number) => {});
      supervisor.on('restarting', restartingHandler);

      supervisor.start();

      // Wait for first restart attempt
      await new Promise((r) => setTimeout(r, 50));

      expect(restartingHandler).toHaveBeenCalled();

      supervisor.stop();
    });

    it('transitions to failed after max retries exhausted', async () => {
      const supervisor = createAudioSupervisor({
        pwCatPath: '/bin/false',
        backoff: {
          baseDelayMs: 5,
          maxDelayMs: 100,
          maxRetries: 2,
          jitterFactor: 0,
        },
      });

      const failedHandler = mock((_err: Error) => {});
      supervisor.on('failed', failedHandler);

      const restartHandler = mock(() => {});
      supervisor.on('restarting', restartHandler);

      supervisor.start();

      // Wait for all retries: initial + 2 restart attempts
      // Delays: 5ms (attempt 0), 10ms (attempt 1), then fail
      // Allow plenty of time for process spawning overhead
      await new Promise((r) => setTimeout(r, 500));

      // Should have made 2 restart attempts before failing
      expect(restartHandler.mock.calls.length).toBe(2);
      expect(failedHandler).toHaveBeenCalled();
      expect(supervisor.getState()).toBe('failed');

      supervisor.stop();
    });

    it('intentional stop prevents restart', async () => {
      const supervisor = createAudioSupervisor({
        pwCatPath: '/bin/false',
        backoff: {
          baseDelayMs: 100, // Long delay to ensure we can stop before restart
          maxRetries: 5,
          jitterFactor: 0,
        },
      });

      const restartCalls: number[] = [];
      supervisor.on('restarting', (attempt) => restartCalls.push(attempt));

      supervisor.start();

      // Wait a bit for first failure and restarting event
      await new Promise((r) => setTimeout(r, 30));

      // Stop before the delayed restart happens
      supervisor.stop();

      const restartCountAtStop = restartCalls.length;

      // Wait to ensure no more restarts happen
      await new Promise((r) => setTimeout(r, 200));

      // No new restarts should have occurred after stop
      expect(restartCalls.length).toBe(restartCountAtStop);
      expect(supervisor.getState()).toBe('stopped');
    });
  });

  describe('successful process lifecycle', () => {
    it('emits started when process spawns successfully', async () => {
      // Use a command that runs briefly - head will exit cleanly
      const supervisor = createAudioSupervisor({
        pwCatPath: '/bin/true',
        backoff: { maxRetries: 0 },
      });

      const startedHandler = mock(() => {});
      supervisor.on('started', startedHandler);

      supervisor.start();

      // Wait for spawn
      await new Promise((r) => setTimeout(r, 30));

      expect(startedHandler).toHaveBeenCalled();

      supervisor.stop();
    });
  });

  describe('factory function', () => {
    it('creates supervisor with custom options', () => {
      const supervisor = createAudioSupervisor({
        pwCatPath: '/custom/path',
        backoff: { maxRetries: 10 },
      });

      expect(supervisor).toBeInstanceOf(AudioSupervisor);
    });

    it('creates supervisor with defaults', () => {
      const supervisor = createAudioSupervisor();
      expect(supervisor).toBeInstanceOf(AudioSupervisor);
    });
  });
});
