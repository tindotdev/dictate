/**
 * Idle Exit Policy Tests
 *
 * Tests the daemon's idle-exit behavior:
 * - Exits after configurable timeout when no clients connected
 * - Timeout can be disabled with DICTATE_IDLE_TIMEOUT_MS=0
 * - Timer is cancelled when client connects or dictation starts
 */

import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	jest,
	spyOn,
} from "bun:test";
import {
	createIdleExitPolicy,
	DEFAULT_IDLE_TIMEOUT_MS,
	type IdleExitPolicy,
	loadIdleTimeoutFromEnv,
} from "../../lifecycle/idle-exit.js";

describe("Idle Exit Policy", () => {
	let exitCalled: boolean;
	let isIdleValue: boolean;
	let policy: IdleExitPolicy;

	beforeEach(() => {
		exitCalled = false;
		isIdleValue = true;
		jest.useFakeTimers();
	});

	afterEach(() => {
		policy?.cancel();
		jest.useRealTimers();
	});

	function createPolicy(timeoutMs: number): IdleExitPolicy {
		policy = createIdleExitPolicy({
			timeoutMs,
			isIdle: () => isIdleValue,
			onExit: () => {
				exitCalled = true;
			},
		});
		return policy;
	}

	describe("basic timeout behavior", () => {
		it("calls onExit after timeout when idle", () => {
			createPolicy(1000);

			// Simulate client disconnect (triggers scheduleExitIfIdle)
			policy.onClientDisconnect();

			// Advance time to just before timeout
			jest.advanceTimersByTime(999);
			expect(exitCalled).toBe(false);

			// Advance past timeout
			jest.advanceTimersByTime(2);
			expect(exitCalled).toBe(true);
		});

		it("does not call onExit if not idle", () => {
			createPolicy(1000);
			isIdleValue = false; // Not idle (e.g., has clients or is listening)

			policy.onClientDisconnect();

			jest.advanceTimersByTime(2000);
			expect(exitCalled).toBe(false);
		});

		it("does not schedule exit when timeout is 0 (disabled)", () => {
			createPolicy(0);

			policy.onClientDisconnect();

			jest.advanceTimersByTime(100000);
			expect(exitCalled).toBe(false);
		});

		it("does not schedule exit when timeout is negative", () => {
			createPolicy(-1000);

			policy.onClientDisconnect();

			jest.advanceTimersByTime(100000);
			expect(exitCalled).toBe(false);
		});
	});

	describe("timer cancellation", () => {
		it("cancels timer when client connects", () => {
			createPolicy(1000);

			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);

			// Client connects - should cancel pending exit
			policy.onClientConnect();

			jest.advanceTimersByTime(1000);
			expect(exitCalled).toBe(false);
		});

		it("cancels timer when dictation starts", () => {
			createPolicy(1000);

			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);

			// Dictation starts - should cancel pending exit
			policy.onDictationStart();

			jest.advanceTimersByTime(1000);
			expect(exitCalled).toBe(false);
		});

		it("cancels timer via cancel() method", () => {
			createPolicy(1000);

			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);

			policy.cancel();

			jest.advanceTimersByTime(1000);
			expect(exitCalled).toBe(false);
		});
	});

	describe("re-scheduling", () => {
		it("can reschedule after cancellation", () => {
			createPolicy(1000);

			// First cycle: schedule -> cancel
			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);
			policy.onClientConnect();

			// Second cycle: reschedule
			policy.onClientDisconnect();
			jest.advanceTimersByTime(1001);

			expect(exitCalled).toBe(true);
		});

		it("reschedules on dictation stop if idle", () => {
			createPolicy(1000);

			policy.onDictationStop();

			jest.advanceTimersByTime(1001);
			expect(exitCalled).toBe(true);
		});

		it("does not double-schedule if timer already pending", () => {
			createPolicy(1000);

			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);

			// Another disconnect (shouldn't reset the timer)
			policy.onClientDisconnect();

			// Should still exit after the original 1000ms, not 1500ms
			jest.advanceTimersByTime(501);
			expect(exitCalled).toBe(true);
		});
	});

	describe("re-check before exit", () => {
		it("does not exit if isIdle becomes false during timeout", () => {
			createPolicy(1000);

			policy.onClientDisconnect();
			jest.advanceTimersByTime(500);

			// Something changed - no longer idle
			isIdleValue = false;

			jest.advanceTimersByTime(600);
			expect(exitCalled).toBe(false);
		});
	});
});

describe("loadIdleTimeoutFromEnv", () => {
	const originalEnv = process.env;

	beforeEach(() => {
		process.env = { ...originalEnv };
	});

	afterEach(() => {
		process.env = originalEnv;
	});

	it("returns default when env var is not set", () => {
		delete process.env.DICTATE_IDLE_TIMEOUT_MS;
		expect(loadIdleTimeoutFromEnv()).toBe(DEFAULT_IDLE_TIMEOUT_MS);
	});

	it("returns default when env var is empty", () => {
		process.env.DICTATE_IDLE_TIMEOUT_MS = "";
		expect(loadIdleTimeoutFromEnv()).toBe(DEFAULT_IDLE_TIMEOUT_MS);
	});

	it("returns 0 when env var is explicitly 0", () => {
		process.env.DICTATE_IDLE_TIMEOUT_MS = "0";
		expect(loadIdleTimeoutFromEnv()).toBe(0);
	});

	it("parses valid positive integer", () => {
		process.env.DICTATE_IDLE_TIMEOUT_MS = "5000";
		expect(loadIdleTimeoutFromEnv()).toBe(5000);
	});

	it("returns default for invalid value (non-numeric)", () => {
		const warnSpy = spyOn(console, "warn").mockImplementation(() => {});
		process.env.DICTATE_IDLE_TIMEOUT_MS = "invalid";
		expect(loadIdleTimeoutFromEnv()).toBe(DEFAULT_IDLE_TIMEOUT_MS);
		expect(warnSpy).toHaveBeenCalledWith(
			`Invalid DICTATE_IDLE_TIMEOUT_MS value "invalid", using default ${DEFAULT_IDLE_TIMEOUT_MS}ms`,
		);
		warnSpy.mockRestore();
	});

	it("returns default for negative value", () => {
		const warnSpy = spyOn(console, "warn").mockImplementation(() => {});
		process.env.DICTATE_IDLE_TIMEOUT_MS = "-1000";
		expect(loadIdleTimeoutFromEnv()).toBe(DEFAULT_IDLE_TIMEOUT_MS);
		expect(warnSpy).toHaveBeenCalledWith(
			`Invalid DICTATE_IDLE_TIMEOUT_MS value "-1000", using default ${DEFAULT_IDLE_TIMEOUT_MS}ms`,
		);
		warnSpy.mockRestore();
	});

	it("handles large values", () => {
		process.env.DICTATE_IDLE_TIMEOUT_MS = "3600000"; // 1 hour
		expect(loadIdleTimeoutFromEnv()).toBe(3600000);
	});
});
