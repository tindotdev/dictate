import { EventEmitter } from "node:events";
import type { DaemonState } from "./protocol.js";

// ============================================================================
// State Events (triggers for transitions)
// ============================================================================

export type StateEvent =
	| { type: "START_LISTENING" }
	| { type: "AUDIO_READY" }
	| { type: "WS_READY" }
	| { type: "STOP_LISTENING" }
	| { type: "FINAL_TRANSCRIPT_RECEIVED" }
	| { type: "WS_DISCONNECTED" }
	| { type: "WS_RECONNECTED" }
	| { type: "FATAL_ERROR"; message: string }
	| { type: "RESET" };

// ============================================================================
// State Machine
// ============================================================================

export interface DaemonStateMachineEvents {
	transition: (from: DaemonState, to: DaemonState, event: StateEvent) => void;
	error: (message: string) => void;
}

export declare interface DaemonStateMachine {
	on<K extends keyof DaemonStateMachineEvents>(
		event: K,
		listener: DaemonStateMachineEvents[K],
	): this;
	emit<K extends keyof DaemonStateMachineEvents>(
		event: K,
		...args: Parameters<DaemonStateMachineEvents[K]>
	): boolean;
}

export class DaemonStateMachine extends EventEmitter {
	private state: DaemonState = "idle";
	private audioReady = false;
	private wsReady = false;

	getState(): DaemonState {
		return this.state;
	}

	isAudioReady(): boolean {
		return this.audioReady;
	}

	isWsReady(): boolean {
		return this.wsReady;
	}

	/**
	 * Process a state event and transition if valid.
	 * Returns true if transition occurred, false otherwise.
	 */
	transition(event: StateEvent): boolean {
		const from = this.state;
		let to: DaemonState | null = null;

		switch (event.type) {
			case "START_LISTENING":
				if (this.state === "idle") {
					to = "audio_starting";
				}
				break;

			case "AUDIO_READY":
				this.audioReady = true;
				if (this.state === "audio_starting" && this.wsReady) {
					to = "listening";
				}
				break;

			case "WS_READY":
				this.wsReady = true;
				if (this.state === "audio_starting" && this.audioReady) {
					to = "listening";
				} else if (this.state === "reconnecting") {
					to = "listening";
				}
				break;

			case "STOP_LISTENING":
				if (this.state === "listening") {
					to = "flushing";
				}
				break;

			case "FINAL_TRANSCRIPT_RECEIVED":
				if (this.state === "flushing") {
					to = "idle";
					this.audioReady = false;
					this.wsReady = false;
				}
				break;

			case "WS_DISCONNECTED":
				this.wsReady = false;
				// Can reconnect from listening or flushing
				if (this.state === "listening" || this.state === "flushing") {
					to = "reconnecting";
				}
				break;

			case "WS_RECONNECTED":
				this.wsReady = true;
				if (this.state === "reconnecting") {
					to = "listening";
				}
				break;

			case "FATAL_ERROR":
				// Can transition to error from any state
				to = "error";
				this.audioReady = false;
				this.wsReady = false;
				this.emit("error", event.message);
				break;

			case "RESET":
				// Reset to idle from any state
				to = "idle";
				this.audioReady = false;
				this.wsReady = false;
				break;
		}

		if (to !== null && to !== from) {
			this.state = to;
			this.emit("transition", from, to, event);
			return true;
		}

		return false;
	}

	/**
	 * Check if a transition is valid without executing it.
	 */
	canTransition(event: StateEvent): boolean {
		// Create a temporary copy to test
		const tempMachine = new DaemonStateMachine();
		tempMachine.state = this.state;
		tempMachine.audioReady = this.audioReady;
		tempMachine.wsReady = this.wsReady;

		// Suppress events on temp machine
		tempMachine.removeAllListeners();

		return tempMachine.transition(event);
	}
}

// ============================================================================
// Factory function for easier testing
// ============================================================================

export function createStateMachine(): DaemonStateMachine {
	return new DaemonStateMachine();
}
