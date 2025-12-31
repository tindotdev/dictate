import { describe, expect, it } from 'bun:test';
import {
  calculateBackoffDelay,
  shouldRetry,
  createBackoffState,
  nextBackoff,
  resetBackoff,
  DEFAULT_BACKOFF_CONFIG,
} from '../backoff.js';

describe('calculateBackoffDelay', () => {
  it('uses default config when none provided', () => {
    // With random=0, no jitter is added
    const delay = calculateBackoffDelay(0, {}, 0);
    expect(delay).toBe(DEFAULT_BACKOFF_CONFIG.baseDelayMs);
  });

  it('doubles delay with each attempt (exponential)', () => {
    const config = { baseDelayMs: 1000, maxDelayMs: 100000, jitterFactor: 0 };

    expect(calculateBackoffDelay(0, config, 0)).toBe(1000); // 1000 * 2^0 = 1000
    expect(calculateBackoffDelay(1, config, 0)).toBe(2000); // 1000 * 2^1 = 2000
    expect(calculateBackoffDelay(2, config, 0)).toBe(4000); // 1000 * 2^2 = 4000
    expect(calculateBackoffDelay(3, config, 0)).toBe(8000); // 1000 * 2^3 = 8000
    expect(calculateBackoffDelay(4, config, 0)).toBe(16000); // 1000 * 2^4 = 16000
  });

  it('caps delay at maxDelayMs', () => {
    const config = { baseDelayMs: 1000, maxDelayMs: 5000, jitterFactor: 0 };

    expect(calculateBackoffDelay(0, config, 0)).toBe(1000);
    expect(calculateBackoffDelay(1, config, 0)).toBe(2000);
    expect(calculateBackoffDelay(2, config, 0)).toBe(4000);
    expect(calculateBackoffDelay(3, config, 0)).toBe(5000); // Capped
    expect(calculateBackoffDelay(10, config, 0)).toBe(5000); // Still capped
  });

  it('applies jitter correctly', () => {
    const config = { baseDelayMs: 1000, maxDelayMs: 100000, jitterFactor: 0.2 };

    // random=0 -> no jitter added
    expect(calculateBackoffDelay(0, config, 0)).toBe(1000);

    // random=0.5 -> half of jitter factor applied: 1000 * (1 + 0.5 * 0.2) = 1100
    expect(calculateBackoffDelay(0, config, 0.5)).toBe(1100);

    // random=1 -> full jitter factor applied: 1000 * (1 + 1 * 0.2) = 1200
    expect(calculateBackoffDelay(0, config, 1)).toBe(1200);
  });

  it('clamps random value to [0, 1]', () => {
    const config = { baseDelayMs: 1000, maxDelayMs: 100000, jitterFactor: 0.2 };

    // Negative random should be treated as 0
    expect(calculateBackoffDelay(0, config, -0.5)).toBe(1000);

    // Random > 1 should be treated as 1
    expect(calculateBackoffDelay(0, config, 1.5)).toBe(1200);
  });

  it('rounds result to integer', () => {
    const config = { baseDelayMs: 1000, maxDelayMs: 100000, jitterFactor: 0.1 };

    // 1000 * (1 + 0.333 * 0.1) = 1033.3 -> 1033
    const delay = calculateBackoffDelay(0, config, 0.333);
    expect(Number.isInteger(delay)).toBe(true);
  });
});

describe('shouldRetry', () => {
  it('returns true when under max retries', () => {
    const config = { maxRetries: 5 };

    expect(shouldRetry(0, config)).toBe(true);
    expect(shouldRetry(1, config)).toBe(true);
    expect(shouldRetry(4, config)).toBe(true);
  });

  it('returns false when at or over max retries', () => {
    const config = { maxRetries: 5 };

    expect(shouldRetry(5, config)).toBe(false);
    expect(shouldRetry(6, config)).toBe(false);
    expect(shouldRetry(100, config)).toBe(false);
  });

  it('returns true always when maxRetries is 0 (infinite)', () => {
    const config = { maxRetries: 0 };

    expect(shouldRetry(0, config)).toBe(true);
    expect(shouldRetry(100, config)).toBe(true);
    expect(shouldRetry(1000000, config)).toBe(true);
  });

  it('uses default config when none provided', () => {
    // Default is 5 retries
    expect(shouldRetry(4)).toBe(true);
    expect(shouldRetry(5)).toBe(false);
  });
});

describe('BackoffState helpers', () => {
  describe('createBackoffState', () => {
    it('creates state with defaults', () => {
      const state = createBackoffState();
      expect(state.attempt).toBe(0);
      expect(state.config).toEqual(DEFAULT_BACKOFF_CONFIG);
    });

    it('merges custom config', () => {
      const state = createBackoffState({ maxRetries: 10, baseDelayMs: 500 });
      expect(state.config.maxRetries).toBe(10);
      expect(state.config.baseDelayMs).toBe(500);
      expect(state.config.maxDelayMs).toBe(DEFAULT_BACKOFF_CONFIG.maxDelayMs);
    });
  });

  describe('nextBackoff', () => {
    it('returns delay and increments attempt', () => {
      const state = createBackoffState({ jitterFactor: 0 });

      const delay1 = nextBackoff(state);
      expect(delay1).toBe(1000);
      expect(state.attempt).toBe(1);

      const delay2 = nextBackoff(state);
      expect(delay2).toBe(2000);
      expect(state.attempt).toBe(2);
    });

    it('returns null when max retries exceeded', () => {
      const state = createBackoffState({ maxRetries: 2, jitterFactor: 0 });

      expect(nextBackoff(state)).toBe(1000); // attempt 0 -> 1
      expect(nextBackoff(state)).toBe(2000); // attempt 1 -> 2
      expect(nextBackoff(state)).toBe(null); // attempt 2, max is 2
      expect(state.attempt).toBe(2); // Not incremented past max
    });
  });

  describe('resetBackoff', () => {
    it('resets attempt counter to 0', () => {
      const state = createBackoffState();
      state.attempt = 5;

      resetBackoff(state);

      expect(state.attempt).toBe(0);
    });

    it('preserves config', () => {
      const state = createBackoffState({ maxRetries: 10 });
      state.attempt = 5;

      resetBackoff(state);

      expect(state.config.maxRetries).toBe(10);
    });
  });
});

describe('integration: typical retry loop', () => {
  it('produces expected delay sequence', () => {
    const state = createBackoffState({
      baseDelayMs: 100,
      maxDelayMs: 1000,
      maxRetries: 5,
      jitterFactor: 0, // No randomness for predictable test
    });

    const delays: (number | null)[] = [];
    for (let i = 0; i < 7; i++) {
      delays.push(nextBackoff(state));
    }

    expect(delays).toEqual([
      100, // attempt 0
      200, // attempt 1
      400, // attempt 2
      800, // attempt 3
      1000, // attempt 4 (capped)
      null, // attempt 5 (max retries)
      null, // attempt 6 (still exceeded)
    ]);
  });

  it('reset allows retries again', () => {
    const state = createBackoffState({
      baseDelayMs: 100,
      maxRetries: 2,
      jitterFactor: 0,
    });

    // Exhaust retries
    nextBackoff(state); // 100
    nextBackoff(state); // 200
    expect(nextBackoff(state)).toBe(null);

    // Reset
    resetBackoff(state);

    // Can retry again
    expect(nextBackoff(state)).toBe(100);
    expect(state.attempt).toBe(1);
  });
});
