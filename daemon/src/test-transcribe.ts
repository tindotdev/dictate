#!/usr/bin/env bun
/**
 * Test script for OpenAI Realtime API transcription
 * Usage: bun run src/test-transcribe.ts <audio.pcm>
 */

import * as path from 'path';
import WebSocket from 'ws';

// Load .env file
const daemonDir = path.dirname(Bun.main);
const envPath = path.join(daemonDir, '..', '.env');
try {
  const envFile = Bun.file(envPath);
  if (await envFile.exists()) {
    const content = await envFile.text();
    for (const line of content.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('#')) continue;
      const eqIndex = trimmed.indexOf('=');
      if (eqIndex === -1) continue;
      const key = trimmed.slice(0, eqIndex).trim();
      const value = trimmed.slice(eqIndex + 1).trim().replace(/^["']|["']$/g, '');
      if (!process.env[key]) {
        process.env[key] = value;
      }
    }
  }
} catch {
  // ignore
}

const OPENAI_API_KEY = process.env.OPENAI_API_KEY;
if (!OPENAI_API_KEY) {
  console.error('Error: OPENAI_API_KEY not set');
  process.exit(1);
}

const audioFile = process.argv[2];
if (!audioFile) {
  console.error('Usage: bun run src/test-transcribe.ts <audio.pcm>');
  process.exit(1);
}

// Read audio file
const audioPath = path.resolve(audioFile);
const audioBuffer = await Bun.file(audioPath).arrayBuffer();
const audioData = Buffer.from(audioBuffer);

console.log(`Audio file: ${audioPath}`);
console.log(`Audio size: ${audioData.length} bytes`);
console.log(`Duration: ~${(audioData.length / (24000 * 2)).toFixed(2)}s (at 24kHz mono s16)`);
console.log('');

// Connect to OpenAI Realtime API
// The URL uses a realtime model, transcription model is set in session config
const realtimeModel = 'gpt-4o-mini-realtime-preview';
const transcriptionModel = process.env.OPENAI_STT_MODEL || 'gpt-4o-mini-transcribe';
const url = `wss://api.openai.com/v1/realtime?model=${realtimeModel}`;

console.log(`Connecting to ${url}...`);
console.log(`Transcription model: ${transcriptionModel}`);

const ws = new WebSocket(url, {
  headers: {
    'Authorization': `Bearer ${OPENAI_API_KEY}`,
    'OpenAI-Beta': 'realtime=v1',
  },
});

ws.on('open', () => {
  console.log('Connected!\n');

  // Send session update for transcription-only mode
  // Configure audio input with transcription settings
  const sessionUpdate = {
    type: 'session.update',
    session: {
      modalities: ['text'],  // Text only, no audio response
      input_audio_format: 'pcm16',
      input_audio_transcription: {
        model: transcriptionModel,
      },
      turn_detection: {
        type: 'server_vad',
        threshold: 0.5,
        prefix_padding_ms: 300,
        silence_duration_ms: 500,
      },
    },
  };

  console.log('Sending session.update for transcription mode...');
  ws.send(JSON.stringify(sessionUpdate));
});

ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  const type = msg.type;

  // Log interesting events
  switch (type) {
    case 'session.created':
      console.log('Session created');
      break;

    case 'session.updated':
      console.log('Session updated, sending audio...\n');
      sendAudio();
      break;

    case 'input_audio_buffer.speech_started':
      console.log(`[Speech Started] item_id: ${msg.item_id}`);
      break;

    case 'input_audio_buffer.speech_stopped':
      console.log(`[Speech Stopped] item_id: ${msg.item_id}`);
      break;

    case 'conversation.item.input_audio_transcription.delta':
      console.log(`[Delta] ${msg.delta}`);
      break;

    case 'conversation.item.input_audio_transcription.completed':
      console.log(`\n[FINAL TRANSCRIPT] "${msg.transcript}"`);
      break;

    case 'error':
      console.error(`[Error] ${msg.error?.message || JSON.stringify(msg)}`);
      break;

    case 'input_audio_buffer.committed':
    case 'input_audio_buffer.cleared':
    case 'conversation.item.created':
    case 'response.created':
    case 'response.done':
      // Ignore these
      break;

    default:
      console.log(`[${type}]`, JSON.stringify(msg).slice(0, 100));
  }
});

ws.on('error', (err) => {
  console.error('WebSocket error:', err.message);
});

ws.on('close', (code, reason) => {
  console.log(`\nConnection closed: ${code} ${reason}`);
  process.exit(0);
});

function sendAudio() {
  // Send audio in 20ms chunks (960 bytes at 24kHz mono s16)
  const CHUNK_SIZE = 960;
  let offset = 0;

  const sendChunk = () => {
    if (offset >= audioData.length) {
      console.log('\nAll audio sent, committing buffer...');
      ws.send(JSON.stringify({ type: 'input_audio_buffer.commit' }));

      // Wait for transcription, then close
      setTimeout(() => {
        console.log('\nClosing connection...');
        ws.close();
      }, 5000);
      return;
    }

    const chunk = audioData.subarray(offset, offset + CHUNK_SIZE);
    const base64 = chunk.toString('base64');

    ws.send(JSON.stringify({
      type: 'input_audio_buffer.append',
      audio: base64,
    }));

    offset += CHUNK_SIZE;

    // Send at roughly real-time pace (20ms per chunk)
    setTimeout(sendChunk, 20);
  };

  sendChunk();
}
