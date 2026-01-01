/**
 * Multi-Client Integration Tests
 *
 * These tests verify that the daemon correctly handles multiple simultaneous
 * client connections, corresponding to TEST_CHECKLIST.md Section 4:
 *
 * 4.1 Multiple dictatectl Instances - Multiple clients can connect simultaneously
 * 4.2 Broadcast to All Clients - Status/transcription events reach all clients
 * 4.3 Client Disconnect - Daemon handles disconnections gracefully
 * 4.4 Multiple Neovim Instances - Duplicate commands handled safely
 *
 * Architecture:
 * - Real SocketServer with temp socket files
 * - Real StateMachine
 * - Mock AudioSupervisor (event emitter, no real pw-cat process)
 * - Mock NetworkSupervisor (connects to local WebSocket server)
 *
 * Run with: bun test src/__tests__/integration/multi-client.test.ts
 */

import { describe, expect, it, beforeEach, afterEach } from 'bun:test';
import * as net from 'net';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { EventEmitter } from 'events';
import { WebSocketServer } from 'ws';

import { createSocketServer, type SocketServer } from '../../server.js';
import { createStateMachine, type DaemonStateMachine } from '../../state-machine.js';
import { createNetworkSupervisor, type NetworkSupervisor } from '../../supervisors/network.js';
import type { ClientMessage, DaemonMessage, DaemonState } from '../../protocol.js';
import type { Config } from '../../config.js';

// ============================================================================
// Mock Audio Supervisor (doesn't spawn real processes)
// ============================================================================

interface MockAudioSupervisor extends EventEmitter {
  start(): void;
  stop(): void;
  isRunning(): boolean;
  getState(): string;
}

function createMockAudioSupervisor(): MockAudioSupervisor {
  const emitter = new EventEmitter() as MockAudioSupervisor;
  let running = false;

  emitter.start = () => {
    if (!running) {
      running = true;
      // Emit started asynchronously like the real supervisor
      setImmediate(() => {
        emitter.emit('started');
      });
    }
  };

  emitter.stop = () => {
    if (running) {
      running = false;
      emitter.emit('stopped');
    }
  };

  emitter.isRunning = () => running;
  emitter.getState = () => (running ? 'running' : 'stopped');

  return emitter;
}

// ============================================================================
// Test Harness: Wires components together like main.ts
// ============================================================================

interface TestDaemon {
  server: SocketServer;
  stateMachine: DaemonStateMachine;
  audio: MockAudioSupervisor;
  network: NetworkSupervisor;
  mockWsServer: WebSocketServer;
  socketPath: string;
  cleanup: () => void;
}

const mockConfig: Config = {
  apiKey: 'test-key',
  model: 'gpt-4o-mini-transcribe',
  vadThreshold: 0.5,
  vadPrefixPaddingMs: 300,
  vadSilenceDurationMs: 500,
};

async function createTestDaemon(wsPort: number): Promise<TestDaemon> {
  // Create temp directory for socket
  const testDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dictate-integration-'));
  const socketPath = path.join(testDir, 'dictate.sock');

  // Create mock WebSocket server
  const mockWsServer = await new Promise<WebSocketServer>((resolve) => {
    const wss = new WebSocketServer({ port: wsPort });
    wss.on('listening', () => resolve(wss));
  });

  // Create components
  const server = createSocketServer();
  const stateMachine = createStateMachine();
  const audio = createMockAudioSupervisor();
  const network = createNetworkSupervisor({
    config: mockConfig,
    wsUrl: `ws://localhost:${wsPort}`,
    backoff: { maxRetries: 1, baseDelayMs: 10, jitterFactor: 0 },
  });

  // Wire up event handlers (replicating main.ts logic)

  // State machine -> broadcast status
  stateMachine.on('transition', () => {
    server.broadcast({
      type: 'status',
      state: stateMachine.getState(),
      audio_ok: audio.isRunning(),
      ws_ok: network.isConnected(),
    });
  });

  // Audio supervisor -> state machine
  audio.on('started', () => {
    stateMachine.transition({ type: 'AUDIO_READY' });
  });

  // Network supervisor -> state machine
  network.on('connected', () => {
    stateMachine.transition({ type: 'WS_READY' });
  });

  network.on('disconnected', () => {
    // Only transition if we're in a connected state
    if (stateMachine.getState() === 'listening') {
      stateMachine.transition({ type: 'WS_DISCONNECTED' });
    }
  });

  // Network events -> broadcast to clients
  network.on('speech_started', (itemId) => {
    server.broadcast({ type: 'speech_started', item_id: itemId });
  });

  network.on('delta', (itemId, text) => {
    server.broadcast({ type: 'partial_transcript', item_id: itemId, text });
  });

  network.on('completed', (itemId, transcript) => {
    server.broadcast({ type: 'final_transcript', item_id: itemId, text: transcript });
    if (stateMachine.getState() === 'flushing') {
      stateMachine.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
    }
  });

  // Socket server -> handle client messages
  server.on('client_connected', (clientId) => {
    // Send current status to new client
    server.send(clientId, {
      type: 'status',
      state: stateMachine.getState(),
      audio_ok: audio.isRunning(),
      ws_ok: network.isConnected(),
    });
  });

  server.on('client_message', (_clientId, msg) => {
    switch (msg.type) {
      case 'start_listening':
      case 'start': {
        const state = stateMachine.getState();
        if (state === 'listening' || state === 'error') {
          return; // Already listening or in error state
        }
        const started = stateMachine.transition({ type: 'START_LISTENING' });
        if (started) {
          network.connect();
          audio.start();
        }
        break;
      }

      case 'stop_listening':
      case 'stop': {
        const state = stateMachine.getState();
        if (state === 'idle') {
          return; // Already idle
        }
        stateMachine.transition({ type: 'STOP_LISTENING' });
        audio.stop();
        // For tests, immediately transition to idle
        stateMachine.transition({ type: 'FINAL_TRANSCRIPT_RECEIVED' });
        network.disconnect();
        break;
      }
    }
  });

  // Start server
  server.listen({ socketPath });
  await server.ready;

  const cleanup = () => {
    audio.stop();
    network.disconnect();
    server.close();
    mockWsServer.close();
    if (fs.existsSync(testDir)) {
      fs.rmSync(testDir, { recursive: true });
    }
  };

  return {
    server,
    stateMachine,
    audio,
    network,
    mockWsServer,
    socketPath,
    cleanup,
  };
}

// ============================================================================
// Test Client Helpers
// ============================================================================

interface TestClient {
  socket: net.Socket;
  messages: DaemonMessage[];
  send: (msg: ClientMessage) => void;
  waitForMessage: (predicate: (msg: DaemonMessage) => boolean, timeoutMs?: number) => Promise<DaemonMessage>;
  waitForState: (state: DaemonState, timeoutMs?: number) => Promise<DaemonMessage>;
  getLastStatus: () => DaemonMessage | undefined;
  destroy: () => void;
}

function createTestClient(socketPath: string): Promise<TestClient> {
  return new Promise((resolve, reject) => {
    const socket = new net.Socket();
    const messages: DaemonMessage[] = [];
    let buffer = '';
    let messageCount = 0;

    socket.on('data', (data) => {
      buffer += data.toString();
      let newlineIndex: number;
      while ((newlineIndex = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, newlineIndex);
        buffer = buffer.slice(newlineIndex + 1);
        if (line.trim()) {
          try {
            const msg = JSON.parse(line) as DaemonMessage;
            messages.push(msg);
            messageCount++;
          } catch {
            // Ignore parse errors in tests
          }
        }
      }
    });

    socket.on('error', reject);

    socket.connect(socketPath, () => {
      const client: TestClient = {
        socket,
        messages,

        send(msg: ClientMessage) {
          socket.write(JSON.stringify(msg) + '\n');
        },

        waitForMessage(predicate, timeoutMs = 2000) {
          return new Promise((res, rej) => {
            const startCount = messageCount;

            const check = () => {
              // Check all messages, newest first (more likely to match)
              for (let i = messages.length - 1; i >= 0; i--) {
                if (predicate(messages[i])) {
                  return messages[i];
                }
              }
              return null;
            };

            // Check if already received
            const existing = check();
            if (existing) {
              res(existing);
              return;
            }

            const timeout = setTimeout(() => {
              clearInterval(interval);
              rej(new Error(`Timeout waiting for message. Messages received: ${messages.length}, looking for predicate. Last 5: ${JSON.stringify(messages.slice(-5))}`));
            }, timeoutMs);

            const interval = setInterval(() => {
              const found = check();
              if (found) {
                clearTimeout(timeout);
                clearInterval(interval);
                res(found);
              }
            }, 10);
          });
        },

        waitForState(state, timeoutMs = 2000) {
          return this.waitForMessage(
            (msg) => msg.type === 'status' && msg.state === state,
            timeoutMs
          );
        },

        getLastStatus() {
          for (let i = messages.length - 1; i >= 0; i--) {
            if (messages[i].type === 'status') {
              return messages[i];
            }
          }
          return undefined;
        },

        destroy() {
          socket.destroy();
        },
      };

      resolve(client);
    });
  });
}

// Helper to wait for a condition
function waitFor(conditionFn: () => boolean, timeoutMs = 1000): Promise<void> {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const check = () => {
      if (conditionFn()) {
        resolve();
      } else if (Date.now() - start > timeoutMs) {
        reject(new Error('Timeout waiting for condition'));
      } else {
        setTimeout(check, 10);
      }
    };
    check();
  });
}

// ============================================================================
// Tests
// ============================================================================

describe('Multi-Client Integration', () => {
  let daemon: TestDaemon;
  let clients: TestClient[] = [];
  let wsPort = 19100; // Start port for mock WS servers

  beforeEach(async () => {
    wsPort++; // Use different port for each test
    daemon = await createTestDaemon(wsPort);
    clients = [];
  });

  afterEach(() => {
    for (const client of clients) {
      client.destroy();
    }
    clients = [];
    daemon.cleanup();
  });

  async function connectClient(): Promise<TestClient> {
    const client = await createTestClient(daemon.socketPath);
    clients.push(client);
    return client;
  }

  // ==========================================================================
  // Test 4.1: Multiple dictatectl Instances Connect
  // ==========================================================================

  describe('4.1 Multiple clients connecting', () => {
    it('accepts multiple simultaneous connections', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();
      const client3 = await connectClient();

      await waitFor(() => daemon.server.getClientCount() === 3);

      expect(daemon.server.getClientCount()).toBe(3);
      expect(daemon.server.getClientIds().length).toBe(3);

      // All should be connected
      expect(client1.socket.destroyed).toBe(false);
      expect(client2.socket.destroyed).toBe(false);
      expect(client3.socket.destroyed).toBe(false);
    });

    it('sends initial status to each connecting client', async () => {
      const client1 = await connectClient();

      // Wait for initial status
      const status1 = await client1.waitForState('idle');
      expect(status1.type).toBe('status');
      expect(status1.state).toBe('idle');

      const client2 = await connectClient();
      const status2 = await client2.waitForState('idle');
      expect(status2.type).toBe('status');
      expect(status2.state).toBe('idle');
    });

    it('assigns unique client IDs', async () => {
      const clientIds: string[] = [];
      daemon.server.on('client_connected', (id) => clientIds.push(id));

      await connectClient();
      await connectClient();
      await connectClient();

      await waitFor(() => clientIds.length === 3);

      // All IDs should be unique
      const uniqueIds = new Set(clientIds);
      expect(uniqueIds.size).toBe(3);

      // IDs should follow pattern
      for (const id of clientIds) {
        expect(id).toMatch(/^client_\d+$/);
      }
    });
  });

  // ==========================================================================
  // Test 4.2: Broadcast to All Clients
  // ==========================================================================

  describe('4.2 Broadcast status to all clients', () => {
    it('broadcasts state changes to all connected clients', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      // Wait for initial status
      await client1.waitForState('idle');
      await client2.waitForState('idle');

      // Client 1 sends start_listening
      client1.send({ type: 'start_listening' });

      // Wait a bit for processing
      await new Promise((r) => setTimeout(r, 100));

      // Both clients should have received audio_starting (check in message history)
      const hasAudioStarting1 = client1.messages.some(
        (m) => m.type === 'status' && m.state === 'audio_starting'
      );
      const hasAudioStarting2 = client2.messages.some(
        (m) => m.type === 'status' && m.state === 'audio_starting'
      );

      expect(hasAudioStarting1).toBe(true);
      expect(hasAudioStarting2).toBe(true);
    });

    it('broadcasts listening state when both audio and WS are ready', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      await client2.waitForState('idle');

      client1.send({ type: 'start_listening' });

      // Both should eventually reach listening state
      const [listening1, listening2] = await Promise.all([
        client1.waitForState('listening'),
        client2.waitForState('listening'),
      ]);

      expect(listening1.state).toBe('listening');
      expect(listening1.audio_ok).toBe(true);
      expect(listening1.ws_ok).toBe(true);

      expect(listening2.state).toBe('listening');
      expect(listening2.audio_ok).toBe(true);
      expect(listening2.ws_ok).toBe(true);
    });

    it('broadcasts transcription events to all clients', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');

      // Simulate OpenAI sending transcription events via mock WS server
      daemon.mockWsServer.clients.forEach((ws) => {
        ws.send(JSON.stringify({
          type: 'input_audio_buffer.speech_started',
          item_id: 'test_item_123',
        }));
      });

      // Both clients should receive speech_started
      const [speech1, speech2] = await Promise.all([
        client1.waitForMessage((m) => m.type === 'speech_started'),
        client2.waitForMessage((m) => m.type === 'speech_started'),
      ]);

      expect(speech1.type).toBe('speech_started');
      expect((speech1 as { item_id: string }).item_id).toBe('test_item_123');
      expect(speech2.type).toBe('speech_started');
      expect((speech2 as { item_id: string }).item_id).toBe('test_item_123');
    });

    it('broadcasts partial transcripts to all clients', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');

      // Simulate partial transcript
      daemon.mockWsServer.clients.forEach((ws) => {
        ws.send(JSON.stringify({
          type: 'conversation.item.input_audio_transcription.delta',
          item_id: 'test_item_123',
          delta: 'hello world',
        }));
      });

      const [delta1, delta2] = await Promise.all([
        client1.waitForMessage((m) => m.type === 'partial_transcript'),
        client2.waitForMessage((m) => m.type === 'partial_transcript'),
      ]);

      expect((delta1 as { text: string }).text).toBe('hello world');
      expect((delta2 as { text: string }).text).toBe('hello world');
    });

    it('broadcasts final transcripts to all clients', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');

      // Simulate final transcript
      daemon.mockWsServer.clients.forEach((ws) => {
        ws.send(JSON.stringify({
          type: 'conversation.item.input_audio_transcription.completed',
          item_id: 'test_item_123',
          transcript: 'Hello, world!',
        }));
      });

      const [final1, final2] = await Promise.all([
        client1.waitForMessage((m) => m.type === 'final_transcript'),
        client2.waitForMessage((m) => m.type === 'final_transcript'),
      ]);

      expect((final1 as { text: string }).text).toBe('Hello, world!');
      expect((final2 as { text: string }).text).toBe('Hello, world!');
    });
  });

  // ==========================================================================
  // Test 4.3: Client Disconnect Handling
  // ==========================================================================

  describe('4.3 Client disconnect handling', () => {
    it('continues operating when one client disconnects', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      await client2.waitForState('idle');

      // Start listening
      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');
      await client2.waitForState('listening');

      // Disconnect client 1
      client1.destroy();

      await waitFor(() => daemon.server.getClientCount() === 1);

      // Client 2 should still be able to receive events
      const msgCountBefore = client2.messages.length;

      daemon.mockWsServer.clients.forEach((ws) => {
        ws.send(JSON.stringify({
          type: 'input_audio_buffer.speech_started',
          item_id: 'after_disconnect',
        }));
      });

      // Wait for the new message
      await waitFor(() => client2.messages.length > msgCountBefore, 2000);

      const speech = client2.messages.find(
        (m) => m.type === 'speech_started' && (m as { item_id: string }).item_id === 'after_disconnect'
      );
      expect(speech).toBeDefined();
    });

    it('remaining client can send commands after other disconnects', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      await client2.waitForState('idle');

      // Start listening from client 1
      client1.send({ type: 'start_listening' });
      await client2.waitForState('listening');

      // Disconnect client 1
      client1.destroy();
      await waitFor(() => daemon.server.getClientCount() === 1);

      // Client 2 should be able to stop listening
      client2.send({ type: 'stop_listening' });

      // Wait for idle state
      await waitFor(() => daemon.stateMachine.getState() === 'idle', 2000);

      // Verify client2 received idle status
      const hasIdle = client2.messages.some(
        (m) => m.type === 'status' && m.state === 'idle'
      );
      expect(hasIdle).toBe(true);
    });

    it('daemon accepts new connections after all clients disconnect', async () => {
      const client1 = await connectClient();
      await client1.waitForState('idle');

      // Disconnect
      client1.destroy();
      await waitFor(() => daemon.server.getClientCount() === 0);

      // New client should be able to connect
      const client2 = await connectClient();
      const status = await client2.waitForState('idle');

      expect(status.state).toBe('idle');
      expect(daemon.server.getClientCount()).toBe(1);
    });

    it('logs client disconnection', async () => {
      const disconnectedIds: string[] = [];
      daemon.server.on('client_disconnected', (id) => disconnectedIds.push(id));

      const client1 = await connectClient();
      await client1.waitForState('idle');

      const initialCount = daemon.server.getClientCount();
      expect(initialCount).toBe(1);

      client1.destroy();

      await waitFor(() => disconnectedIds.length === 1);
      expect(disconnectedIds[0]).toMatch(/^client_\d+$/);
    });
  });

  // ==========================================================================
  // Test 4.4: Duplicate Command Handling
  // ==========================================================================

  describe('4.4 Duplicate start_listening handling', () => {
    it('handles duplicate start_listening from multiple clients', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      await client2.waitForState('idle');

      // Both clients send start_listening nearly simultaneously
      client1.send({ type: 'start_listening' });
      client2.send({ type: 'start_listening' });

      // Both should eventually reach listening state
      await Promise.all([
        client1.waitForState('listening'),
        client2.waitForState('listening'),
      ]);

      // State machine should be in listening state (not errored)
      expect(daemon.stateMachine.getState()).toBe('listening');
    });

    it('ignores start_listening when already listening', async () => {
      const client1 = await connectClient();
      await client1.waitForState('idle');

      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');

      // Send another start_listening
      const messageCountBefore = client1.messages.length;
      client1.send({ type: 'start_listening' });

      // Wait a bit to ensure no state change
      await new Promise((r) => setTimeout(r, 100));

      // Should still be listening, no additional status messages
      expect(daemon.stateMachine.getState()).toBe('listening');
      // Message count should not have increased significantly
      expect(client1.messages.length).toBeLessThanOrEqual(messageCountBefore + 1);
    });

    it('handles stop_listening from any client', async () => {
      const client1 = await connectClient();
      const client2 = await connectClient();

      await client1.waitForState('idle');
      await client2.waitForState('idle');

      // Client 1 starts listening
      client1.send({ type: 'start_listening' });
      await client1.waitForState('listening');
      await client2.waitForState('listening');

      // Client 2 stops listening (not the one who started)
      client2.send({ type: 'stop_listening' });

      // Wait for state machine to reach idle
      await waitFor(() => daemon.stateMachine.getState() === 'idle', 2000);

      // Verify both clients have idle in their message history
      const client1HasIdle = client1.messages.some(
        (m) => m.type === 'status' && m.state === 'idle'
      );
      const client2HasIdle = client2.messages.some(
        (m) => m.type === 'status' && m.state === 'idle'
      );

      expect(client1HasIdle).toBe(true);
      expect(client2HasIdle).toBe(true);
      expect(daemon.stateMachine.getState()).toBe('idle');
    });

    it('handles rapid start/stop commands', async () => {
      const client1 = await connectClient();
      await client1.waitForState('idle');

      // Rapid toggle
      for (let i = 0; i < 3; i++) {
        client1.send({ type: 'start_listening' });
        await new Promise((r) => setTimeout(r, 50));
        client1.send({ type: 'stop_listening' });
        await new Promise((r) => setTimeout(r, 50));
      }

      // Should eventually settle to idle without crashing
      await new Promise((r) => setTimeout(r, 500));

      const finalState = daemon.stateMachine.getState();
      expect(['idle', 'listening', 'flushing']).toContain(finalState);
    });
  });

  // ==========================================================================
  // Additional Edge Cases
  // ==========================================================================

  describe('Edge cases', () => {
    it('handles client connecting during state transition', async () => {
      const client1 = await connectClient();
      await client1.waitForState('idle');

      // Start listening
      client1.send({ type: 'start_listening' });

      // Connect client2 during the transition
      const client2 = await connectClient();

      // Client2 should receive current state (whatever it is)
      const status = await client2.waitForMessage((m) => m.type === 'status', 2000);
      expect(status.type).toBe('status');
      expect(['idle', 'audio_starting', 'listening']).toContain(status.state);
    });

    it('sends correct audio_ok and ws_ok flags', async () => {
      const client1 = await connectClient();
      const initialStatus = await client1.waitForState('idle');

      // Initially both should be false
      expect(initialStatus.audio_ok).toBe(false);
      expect(initialStatus.ws_ok).toBe(false);

      client1.send({ type: 'start_listening' });

      // Wait for listening state
      const listeningStatus = await client1.waitForState('listening', 2000);
      expect(listeningStatus.audio_ok).toBe(true);
      expect(listeningStatus.ws_ok).toBe(true);
    });
  });
});
