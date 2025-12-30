import WebSocket from 'ws';
import { EventEmitter } from 'events';
import type { Config } from './config.js';
import { emitDebug } from './protocol.js';

const REALTIME_URL = 'wss://api.openai.com/v1/realtime';
// The realtime model for the WebSocket connection (not the transcription model)
const REALTIME_MODEL = 'gpt-4o-mini-realtime-preview';

export interface RealtimeEvents {
  open: [];
  close: [code: number, reason: string];
  error: [error: Error];
  speech_started: [itemId: string];
  speech_stopped: [itemId: string];
  delta: [itemId: string, text: string];
  completed: [itemId: string, transcript: string];
}

export class RealtimeClient extends EventEmitter<RealtimeEvents> {
  private ws: WebSocket | null = null;
  private config: Config;

  constructor(config: Config) {
    super();
    this.config = config;
  }

  connect(): void {
    if (this.ws) {
      emitDebug('WebSocket already connected');
      return;
    }

    const url = `${REALTIME_URL}?model=${REALTIME_MODEL}`;
    emitDebug(`Connecting to ${url} (transcription model: ${this.config.model})`);

    this.ws = new WebSocket(url, {
      headers: {
        Authorization: `Bearer ${this.config.apiKey}`,
        'OpenAI-Beta': 'realtime=v1',
      },
    });

    this.ws.on('open', () => {
      emitDebug('WebSocket connected');
      this.sendSessionUpdate();
      this.emit('open');
    });

    this.ws.on('message', (data) => {
      try {
        const event = JSON.parse(data.toString());
        this.handleEvent(event);
      } catch (err) {
        emitDebug(`Failed to parse WebSocket message: ${err}`);
        this.emit('error', err as Error);
      }
    });

    this.ws.on('close', (code, reason) => {
      emitDebug(`WebSocket closed: ${code} ${reason.toString()}`);
      this.ws = null;
      this.emit('close', code, reason.toString());
    });

    this.ws.on('error', (err) => {
      emitDebug(`WebSocket error: ${err.message}`);
      this.emit('error', err);
    });
  }

  private sendSessionUpdate(): void {
    const sessionConfig = {
      type: 'session.update',
      session: {
        modalities: ['text'],  // Transcription-only, no audio response
        input_audio_format: 'pcm16',
        input_audio_transcription: {
          model: this.config.model,  // The transcription model (e.g., gpt-4o-mini-transcribe)
          ...(this.config.prompt && { prompt: this.config.prompt }),
        },
        turn_detection: {
          type: 'server_vad',
          threshold: this.config.vadThreshold,
          prefix_padding_ms: this.config.vadPrefixPaddingMs,
          silence_duration_ms: this.config.vadSilenceDurationMs,
        },
      },
    };

    emitDebug(`Sending session.update: ${JSON.stringify(sessionConfig)}`);
    this.send(sessionConfig);
  }

  private handleEvent(event: { type: string; [key: string]: unknown }): void {
    emitDebug(`Received event: ${event.type}`);

    switch (event.type) {
      case 'session.created':
        emitDebug('Session created');
        break;

      case 'session.updated':
        emitDebug('Session updated');
        break;

      case 'input_audio_buffer.speech_started': {
        const itemId = (event.item_id as string) ?? `item_${Date.now()}`;
        emitDebug(`Speech started: ${itemId}`);
        this.emit('speech_started', itemId);
        break;
      }

      case 'input_audio_buffer.speech_stopped': {
        const itemId = (event.item_id as string) ?? '';
        emitDebug(`Speech stopped: ${itemId}`);
        this.emit('speech_stopped', itemId);
        break;
      }

      case 'conversation.item.input_audio_transcription.delta': {
        const itemId = (event.item_id as string) ?? '';
        const delta = (event.delta as string) ?? '';
        emitDebug(`Delta for ${itemId}: "${delta}"`);
        this.emit('delta', itemId, delta);
        break;
      }

      case 'conversation.item.input_audio_transcription.completed': {
        const itemId = (event.item_id as string) ?? '';
        const transcript = (event.transcript as string) ?? '';
        emitDebug(`Completed for ${itemId}: "${transcript}"`);
        this.emit('completed', itemId, transcript);
        break;
      }

      case 'error': {
        const errorData = event.error as { message?: string; type?: string } | undefined;
        const message = errorData?.message ?? 'Unknown error';
        const errorType = errorData?.type ?? 'unknown';
        emitDebug(`API error: [${errorType}] ${message}`);
        this.emit('error', new Error(`[${errorType}] ${message}`));
        break;
      }

      case 'rate_limits.updated':
        emitDebug(`Rate limits updated: ${JSON.stringify(event.rate_limits)}`);
        break;

      default:
        emitDebug(`Unhandled event type: ${event.type}`);
    }
  }

  sendAudio(base64Audio: string): void {
    if (this.ws?.readyState !== WebSocket.OPEN) {
      return;
    }

    // Basic backpressure check
    if (this.ws.bufferedAmount > 1024 * 1024) {
      emitDebug('WebSocket backpressure, dropping audio frame');
      return;
    }

    this.send({
      type: 'input_audio_buffer.append',
      audio: base64Audio,
    });
  }

  private send(data: unknown): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(data));
    }
  }

  disconnect(): void {
    if (this.ws) {
      emitDebug('Disconnecting WebSocket');
      this.ws.close();
      this.ws = null;
    }
  }

  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }
}
