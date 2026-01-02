// ============================================================================
// Idle Exit Policy
// ============================================================================
// Exits the daemon after a configurable timeout when:
// - No clients are connected
// - Daemon is in idle state (not listening/transcribing)

// ============================================================================
// Types
// ============================================================================

export interface IdleExitOptions {
	/** Exit timeout in ms (0 = disabled, for systemd/always-on users) */
	timeoutMs: number;
	/** Called to check if daemon is idle (0 clients AND state === 'idle') */
	isIdle: () => boolean;
	/** Called when exiting due to idle timeout */
	onExit: () => void;
}

export interface IdleExitPolicy {
	/** Called when a client connects - cancels any pending exit */
	onClientConnect(): void;
	/** Called when a client disconnects - may schedule exit */
	onClientDisconnect(): void;
	/** Called when dictation starts - cancels any pending exit */
	onDictationStart(): void;
	/** Called when dictation stops - may schedule exit if no clients */
	onDictationStop(): void;
	/** Force cancel any pending exit timer */
	cancel(): void;
}

// ============================================================================
// Constants
// ============================================================================

/** Default idle timeout: 60 seconds */
export const DEFAULT_IDLE_TIMEOUT_MS = 60000;

// ============================================================================
// Implementation
// ============================================================================

export function createIdleExitPolicy(options: IdleExitOptions): IdleExitPolicy {
	const { timeoutMs, isIdle, onExit } = options;

	let exitTimer: ReturnType<typeof setTimeout> | null = null;

	function cancelTimer(): void {
		if (exitTimer) {
			clearTimeout(exitTimer);
			exitTimer = null;
		}
	}

	function scheduleExitIfIdle(): void {
		// Don't schedule if disabled
		if (timeoutMs <= 0) {
			return;
		}

		// Don't schedule if not idle
		if (!isIdle()) {
			return;
		}

		// Don't double-schedule
		if (exitTimer) {
			return;
		}

		exitTimer = setTimeout(() => {
			exitTimer = null;
			// Re-check idle state before exiting
			// (in case something changed during the timeout)
			if (isIdle()) {
				onExit();
			}
		}, timeoutMs);
	}

	return {
		onClientConnect(): void {
			// Client connected - cancel any pending exit
			cancelTimer();
		},

		onClientDisconnect(): void {
			// Client disconnected - maybe schedule exit
			scheduleExitIfIdle();
		},

		onDictationStart(): void {
			// Dictation started - cancel any pending exit
			cancelTimer();
		},

		onDictationStop(): void {
			// Dictation stopped - maybe schedule exit
			scheduleExitIfIdle();
		},

		cancel(): void {
			cancelTimer();
		},
	};
}

/**
 * Load idle timeout from environment variable.
 * Returns DEFAULT_IDLE_TIMEOUT_MS if not set or invalid.
 * Returns 0 if explicitly set to 0 (disables idle exit).
 */
export function loadIdleTimeoutFromEnv(): number {
	const envValue = process.env.DICTATE_IDLE_TIMEOUT_MS;

	if (envValue === undefined || envValue === "") {
		return DEFAULT_IDLE_TIMEOUT_MS;
	}

	const parsed = Number.parseInt(envValue, 10);

	if (Number.isNaN(parsed) || parsed < 0) {
		console.warn(
			`Invalid DICTATE_IDLE_TIMEOUT_MS value "${envValue}", using default ${DEFAULT_IDLE_TIMEOUT_MS}ms`,
		);
		return DEFAULT_IDLE_TIMEOUT_MS;
	}

	return parsed;
}
