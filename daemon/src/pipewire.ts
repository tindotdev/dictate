import { EventEmitter } from 'events';
import type { Subprocess } from 'bun';
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

/**
 * @deprecated Use AudioSupervisor from './supervisors/audio.ts' instead
 */
export class AudioCapture extends EventEmitter<AudioCaptureEvents> {
  private process: Subprocess | null = null;
  private buffer: Buffer = Buffer.alloc(0);
  private intentionalStop: boolean = false;
  private stdoutReader: ReadableStreamDefaultReader<Uint8Array> | null = null;

  start(): void {
    if (this.process) {
      emitDebug('AudioCapture already running');
      return;
    }

    this.intentionalStop = false;
    emitDebug(`Starting pw-cat with ${SAMPLE_RATE}Hz, ${CHANNELS}ch, s16`);

    // pw-cat --record --raw --rate=24000 --channels=1 --format=s16 -
    this.process = Bun.spawn([
      'pw-cat',
      '--record',
      '--raw',
      `--rate=${SAMPLE_RATE}`,
      `--channels=${CHANNELS}`,
      '--format=s16',
      '-',
    ], {
      stdout: 'pipe',
      stderr: 'pipe',
      onExit: (proc, exitCode, signalCode, error) => {
        emitDebug(`pw-cat closed with code ${exitCode}`);
        this.stdoutReader = null;
        this.process = null;
        this.buffer = Buffer.alloc(0);

        if (error) {
          emitDebug(`pw-cat error: ${error.message}`);
          this.emit('error', error);
        }

        this.emit('close', exitCode);
      },
    });

    // Start reading stdout asynchronously
    this.readStdout();

    // Read stderr for debug logging
    this.readStderr();
  }

  private async readStdout(): Promise<void> {
    if (!this.process?.stdout) return;

    try {
      this.stdoutReader = this.process.stdout.getReader();

      while (true) {
        const { done, value } = await this.stdoutReader.read();
        if (done) break;

        this.buffer = Buffer.concat([this.buffer, Buffer.from(value)]);

        // Emit complete frames
        while (this.buffer.length >= FRAME_BYTES) {
          const frame = this.buffer.subarray(0, FRAME_BYTES);
          this.buffer = this.buffer.subarray(FRAME_BYTES);
          this.emit('chunk', frame);
        }
      }
    } catch (err) {
      // Reader cancelled or error - ignore if intentional stop
      if (!this.intentionalStop && err instanceof Error) {
        this.emit('error', err);
      }
    }
  }

  private async readStderr(): Promise<void> {
    if (!this.process?.stderr) return;

    try {
      const reader = this.process.stderr.getReader();

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        const msg = Buffer.from(value).toString().trim();
        if (msg) {
          emitDebug(`pw-cat stderr: ${msg}`);
        }
      }
    } catch {
      // Ignore stderr read errors
    }
  }

  stop(): void {
    if (this.process) {
      emitDebug('Stopping pw-cat');
      this.intentionalStop = true;

      // Cancel the stdout reader first
      if (this.stdoutReader) {
        this.stdoutReader.cancel().catch(() => {});
        this.stdoutReader = null;
      }

      this.process.kill();
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
