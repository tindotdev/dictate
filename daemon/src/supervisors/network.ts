import { EventEmitter } from 'events';
import type { Config } from '../config.js';
import {
  type BackoffConfig,
  createBackoffState,
  nextBackoff,
  resetBackoff,
  type BackoffState,
} from '../backoff.js';

// ============================================================================
// Constants
// ============================================================================

const REALTIME_URL = 'wss://api.openai.com/v1/realtime';

// ============================================================================
// Supervisor Types
// ============================================================================

export type NetworkSupervisorState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'failed';

export interface NetworkSupervisorEvents {
  // State events
  state_change: (from: NetworkSupervisorState, to: NetworkSupervisorState) => void;
  connected: () => void;
  disconnected: () => void;
  reconnecting: (attempt: number, delayMs: number) => void;
  failed: (error: Error) => void;

  // WebSocket events
  error: (error: Error) => void;

  // Realtime API events
  speech_started: (itemId: string) => void;
  speech_stopped: (itemId: string) => void;
  delta: (itemId: string, text: string) => void;
  completed: (itemId: string, transcript: string) => void;
}

export declare interface NetworkSupervisor {
  on<K extends keyof NetworkSupervisorEvents>(event: K, listener: NetworkSupervisorEvents[K]): this;
  emit<K extends keyof NetworkSupervisorEvents>(
    event: K,
    ...args: Parameters<NetworkSupervisorEvents[K]>
  ): boolean;
}

export interface NetworkSupervisorOptions {
  config: Config;
  backoff?: Partial<BackoffConfig>;
  /** Override WebSocket URL for testing */
  wsUrl?: string;
}

// ============================================================================
// Network Supervisor
// ============================================================================

export class NetworkSupervisor extends EventEmitter {
  private ws: WebSocket | null = null;
  private state: NetworkSupervisorState = 'disconnected';
  private intentionalDisconnect = false;
  private backoffState: BackoffState;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private config: Config;
  private wsUrl: string;

  constructor(options: NetworkSupervisorOptions) {
    super();
    this.config = options.config;
    this.backoffState = createBackoffState(options.backoff);
    this.wsUrl = options.wsUrl ?? REALTIME_URL;
  }

  private setState(newState: NetworkSupervisorState): void {
    if (this.state !== newState) {
      const from = this.state;
      this.state = newState;
      this.emit('state_change', from, newState);
    }
  }

  getState(): NetworkSupervisorState {
    return this.state;
  }

  isConnected(): boolean {
    return this.state === 'connected';
  }

  /**
   * Connect to the WebSocket. If already connecting/connected, this is a no-op.
   */
  connect(): void {
    if (this.state === 'connected' || this.state === 'connecting') {
      return;
    }

    this.intentionalDisconnect = false;
    this.cancelReconnect();
    this.doConnect();
  }

  /**
   * Disconnect from the WebSocket. Prevents automatic reconnection.
   */
  disconnect(): void {
    this.intentionalDisconnect = true;
    this.cancelReconnect();
    this.closeWebSocket();
    resetBackoff(this.backoffState);

    if (this.state !== 'disconnected') {
      this.setState('disconnected');
      this.emit('disconnected');
    }
  }

  /**
   * Send audio data to the WebSocket.
   */
  sendAudio(base64Audio: string): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      return;
    }

    // Basic backpressure check (bufferedAmount available on browser WebSocket API)
    if (this.ws.bufferedAmount > 1024 * 1024) {
      return; // Drop frame if buffer is too full
    }

    this.send({
      type: 'input_audio_buffer.append',
      audio: base64Audio,
    });
  }

  private doConnect(): void {
    this.setState('connecting');

    try {
      // Bun native WebSocket supports custom headers as a Bun-specific extension
      this.ws = new WebSocket(this.wsUrl, {
        headers: {
          Authorization: `Bearer ${this.config.apiKey}`,
          'OpenAI-Beta': 'realtime=v1',
        },
      });
    } catch (err) {
      this.emit('error', err as Error);
      this.handleClose(-1, 'Connection failed');
      return;
    }

    this.ws.addEventListener('open', () => {
      this.setState('connected');
      resetBackoff(this.backoffState);
      this.sendSessionUpdate();
      this.emit('connected');
    });

    this.ws.addEventListener('message', (event) => {
      try {
        const data = typeof event.data === 'string' ? event.data : event.data.toString();
        const parsed = JSON.parse(data);
        this.handleEvent(parsed);
      } catch (err) {
        this.emit('error', err as Error);
      }
    });

    this.ws.addEventListener('close', (event) => {
      this.ws = null;
      this.handleClose(event.code, event.reason);
    });

    this.ws.addEventListener('error', () => {
      // Browser WebSocket error events don't expose error details
      this.emit('error', new Error('WebSocket error'));
      // Error is usually followed by close, so we handle reconnection in close handler
    });
  }

  private handleClose(code: number, reason: string): void {
    this.ws = null;

    // If intentionally disconnected, don't reconnect
    if (this.intentionalDisconnect) {
      return;
    }

    // Unexpected disconnect - attempt reconnection with backoff
    const delayMs = nextBackoff(this.backoffState);

    if (delayMs === null) {
      // Max retries exceeded
      this.setState('failed');
      this.emit('failed', new Error(`WebSocket failed after ${this.backoffState.config.maxRetries} retries: ${code} ${reason}`));
      return;
    }

    this.setState('reconnecting');
    this.emit('reconnecting', this.backoffState.attempt, delayMs);

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.intentionalDisconnect) {
        this.doConnect();
      }
    }, delayMs);
  }

  private sendSessionUpdate(): void {
    const sessionConfig: {
      type: string;
      session: {
        audio: {
          input: {
            format: { type: string; rate: number };
            transcription: {
              model: string;
              prompt?: string;
            };
            turn_detection: {
              type: string;
              threshold: number;
              prefix_padding_ms: number;
              silence_duration_ms: number;
            };
            noise_reduction?: { type: string } | null;
          };
        };
        include?: string[];
      };
    } = {
      type: 'session.update',
      session: {
        audio: {
          input: {
            format: {
              type: 'audio/pcm',
              rate: 24000,
            },
            transcription: {
              model: this.config.model,
              ...(this.config.prompt && { prompt: this.config.prompt }),
            },
            turn_detection: {
              type: 'server_vad',
              threshold: this.config.vadThreshold,
              prefix_padding_ms: this.config.vadPrefixPaddingMs,
              silence_duration_ms: this.config.vadSilenceDurationMs,
            },
          },
        },
      },
    };

    // Add noise reduction if configured
    if (this.config.noiseReduction) {
      sessionConfig.session.audio.input.noise_reduction = {
        type: this.config.noiseReduction,
      };
    }

    // Add logprobs if configured
    if (this.config.includeLogprobs) {
      sessionConfig.session.include = ['item.input_audio_transcription.logprobs'];
    }

    this.send(sessionConfig);
  }

  private handleEvent(event: { type: string; [key: string]: unknown }): void {
    switch (event.type) {
      case 'input_audio_buffer.speech_started': {
        const itemId = (event.item_id as string) ?? `item_${Date.now()}`;
        this.emit('speech_started', itemId);
        break;
      }

      case 'input_audio_buffer.speech_stopped': {
        const itemId = (event.item_id as string) ?? '';
        this.emit('speech_stopped', itemId);
        break;
      }

      case 'conversation.item.input_audio_transcription.delta': {
        const itemId = (event.item_id as string) ?? '';
        const delta = (event.delta as string) ?? '';
        this.emit('delta', itemId, delta);
        break;
      }

      case 'conversation.item.input_audio_transcription.completed': {
        const itemId = (event.item_id as string) ?? '';
        const transcript = (event.transcript as string) ?? '';
        this.emit('completed', itemId, transcript);
        break;
      }

      case 'error': {
        const errorData = event.error as { message?: string; type?: string } | undefined;
        const message = errorData?.message ?? 'Unknown error';
        const errorType = errorData?.type ?? 'unknown';
        this.emit('error', new Error(`[${errorType}] ${message}`));
        break;
      }

      // session.created, session.updated, rate_limits.updated are handled silently
    }
  }

  private send(data: unknown): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(data));
    }
  }

  private closeWebSocket(): void {
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  private cancelReconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}

// ============================================================================
// Factory function
// ============================================================================

export function createNetworkSupervisor(options: NetworkSupervisorOptions): NetworkSupervisor {
  return new NetworkSupervisor(options);
}
