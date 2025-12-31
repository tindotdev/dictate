import { spawn, ChildProcessWithoutNullStreams } from 'child_process';
import { EventEmitter } from 'events';
import { emitDebug } from './protocol.js';

// Audio constants for OpenAI Realtime API
const SAMPLE_RATE = 24000;
const CHANNELS = 1;
const BYTES_PER_SAMPLE = 2; // s16 = 16-bit = 2 bytes
const FRAME_MS = 20; // 20ms frames
const FRAME_BYTES = (SAMPLE_RATE * BYTES_PER_SAMPLE * CHANNELS * FRAME_MS) / 1000; // 960 bytes

export interface AudioCaptureEvents {
  chunk: [chunk: Buffer];
  error: [error: Error];
  close: [code: number | null];
}

export class AudioCapture extends EventEmitter<AudioCaptureEvents> {
  private process: ChildProcessWithoutNullStreams | null = null;
  private buffer: Buffer = Buffer.alloc(0);
  private intentionalStop: boolean = false;

  start(): void {
    if (this.process) {
      emitDebug('AudioCapture already running');
      return;
    }

    this.intentionalStop = false;
    emitDebug(`Starting pw-cat with ${SAMPLE_RATE}Hz, ${CHANNELS}ch, s16`);

    // pw-cat --record --rate=24000 --channels=1 --format=s16 -
    this.process = spawn('pw-cat', [
      '--record',
      `--rate=${SAMPLE_RATE}`,
      `--channels=${CHANNELS}`,
      '--format=s16',
      '-',
    ]);

    this.process.stdout.on('data', (data: Buffer) => {
      this.buffer = Buffer.concat([this.buffer, data]);

      // Emit complete frames
      while (this.buffer.length >= FRAME_BYTES) {
        const frame = this.buffer.subarray(0, FRAME_BYTES);
        this.buffer = this.buffer.subarray(FRAME_BYTES);
        this.emit('chunk', frame);
      }
    });

    this.process.stderr.on('data', (data: Buffer) => {
      // pw-cat outputs info to stderr, log but don't fail
      const msg = data.toString().trim();
      if (msg) {
        emitDebug(`pw-cat stderr: ${msg}`);
      }
    });

    this.process.on('error', (err) => {
      emitDebug(`pw-cat error: ${err.message}`);
      this.emit('error', err);
    });

    this.process.on('close', (code) => {
      emitDebug(`pw-cat closed with code ${code}`);
      this.process = null;
      this.buffer = Buffer.alloc(0);
      this.emit('close', code);
    });
  }

  stop(): void {
    if (this.process) {
      emitDebug('Stopping pw-cat');
      this.intentionalStop = true;
      this.process.kill('SIGTERM');
      this.process = null;
      this.buffer = Buffer.alloc(0);
    }
  }

  isRunning(): boolean {
    return this.process !== null;
  }

  wasIntentionallyStopped(): boolean {
    return this.intentionalStop;
  }
}

// Export constants for testing
export const AUDIO_CONSTANTS = {
  SAMPLE_RATE,
  CHANNELS,
  BYTES_PER_SAMPLE,
  FRAME_MS,
  FRAME_BYTES,
} as const;
