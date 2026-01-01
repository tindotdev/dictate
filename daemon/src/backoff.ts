// ============================================================================
// Exponential Backoff (Pure Functions)
// ============================================================================

export interface BackoffConfig {
	/** Initial delay in milliseconds (default: 1000) */
	baseDelayMs: number;
	/** Maximum delay cap in milliseconds (default: 30000) */
	maxDelayMs: number;
	/** Give up after N attempts, 0 = infinite (default: 5) */
	maxRetries: number;
	/** Jitter factor 0-1 for randomization (default: 0.1) */
	jitterFactor: number;
}

export const DEFAULT_BACKOFF_CONFIG: BackoffConfig = {
	baseDelayMs: 1000,
	maxDelayMs: 30000,
	maxRetries: 5,
	jitterFactor: 0.1,
};

/**
 * Calculate delay for a given attempt using exponential backoff with jitter.
 * Formula: min(baseDelay * 2^attempt, maxDelay) * (1 + random * jitter)
 *
 * @param attempt - Zero-based attempt number (0 = first retry)
 * @param config - Backoff configuration
 * @param random - Random value 0-1 (injectable for testing, defaults to Math.random())
 * @returns Delay in milliseconds
 */
export function calculateBackoffDelay(
	attempt: number,
	config: Partial<BackoffConfig> = {},
	random: number = Math.random(),
): number {
	const cfg = { ...DEFAULT_BACKOFF_CONFIG, ...config };

	// Clamp random to [0, 1]
	const clampedRandom = Math.max(0, Math.min(1, random));

	// Exponential delay: baseDelay * 2^attempt
	const exponentialDelay = cfg.baseDelayMs * 2 ** attempt;

	// Cap at maxDelay
	const cappedDelay = Math.min(exponentialDelay, cfg.maxDelayMs);

	// Apply jitter: delay * (1 + random * jitterFactor)
	// This adds 0% to jitterFactor*100% to the delay
	const jitteredDelay = cappedDelay * (1 + clampedRandom * cfg.jitterFactor);

	return Math.round(jitteredDelay);
}

/**
 * Check if another retry should be attempted.
 *
 * @param attempt - Zero-based attempt number (0 = first retry)
 * @param config - Backoff configuration
 * @returns true if retry should be attempted
 */
export function shouldRetry(
	attempt: number,
	config: Partial<BackoffConfig> = {},
): boolean {
	const cfg = { ...DEFAULT_BACKOFF_CONFIG, ...config };

	// maxRetries of 0 means infinite retries
	if (cfg.maxRetries === 0) {
		return true;
	}

	return attempt < cfg.maxRetries;
}

/**
 * Create a backoff state tracker for use in async retry loops.
 * Encapsulates attempt counting and delay calculation.
 */
export interface BackoffState {
	attempt: number;
	config: BackoffConfig;
}

export function createBackoffState(
	config: Partial<BackoffConfig> = {},
): BackoffState {
	return {
		attempt: 0,
		config: { ...DEFAULT_BACKOFF_CONFIG, ...config },
	};
}

/**
 * Get next delay and increment attempt counter.
 * Returns null if max retries exceeded.
 */
export function nextBackoff(state: BackoffState): number | null {
	if (!shouldRetry(state.attempt, state.config)) {
		return null;
	}

	const delay = calculateBackoffDelay(state.attempt, state.config);
	state.attempt++;
	return delay;
}

/**
 * Reset backoff state after successful operation.
 */
export function resetBackoff(state: BackoffState): void {
	state.attempt = 0;
}
