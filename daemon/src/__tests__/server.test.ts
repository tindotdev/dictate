import { describe, expect, it, mock, afterEach, beforeEach } from 'bun:test';
import * as net from 'net';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import {
  SocketServer,
  createSocketServer,
  getDefaultSocketPath,
} from '../server.js';
import type { ClientMessage, DaemonMessage } from '../protocol.js';

describe('SocketServer', () => {
  let server: SocketServer;
  let testDir: string;
  let socketPath: string;

  beforeEach(() => {
    // Create a temp directory for each test
    testDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dictate-test-'));
    socketPath = path.join(testDir, 'dictate.sock');
    server = createSocketServer();
  });

  afterEach(() => {
    server.close();
    // Clean up temp directory
    if (fs.existsSync(testDir)) {
      fs.rmSync(testDir, { recursive: true });
    }
  });

  function createClient(): Promise<net.Socket> {
    return new Promise((resolve) => {
      const client = new net.Socket();
      client.connect(socketPath, () => resolve(client));
    });
  }

  function sendMessage(client: net.Socket, message: object): void {
    client.write(JSON.stringify(message) + '\n');
  }

  function waitForMessage(client: net.Socket): Promise<DaemonMessage> {
    return new Promise((resolve) => {
      let buffer = '';
      const handler = (data: Buffer) => {
        buffer += data.toString();
        const newlineIndex = buffer.indexOf('\n');
        if (newlineIndex !== -1) {
          const line = buffer.slice(0, newlineIndex);
          client.off('data', handler);
          resolve(JSON.parse(line));
        }
      };
      client.on('data', handler);
    });
  }

  describe('getDefaultSocketPath', () => {
    it('uses XDG_RUNTIME_DIR when available', () => {
      const original = process.env.XDG_RUNTIME_DIR;
      process.env.XDG_RUNTIME_DIR = '/run/user/1000';

      const result = getDefaultSocketPath();
      expect(result).toBe('/run/user/1000/dictate/dictate.sock');

      if (original) {
        process.env.XDG_RUNTIME_DIR = original;
      } else {
        delete process.env.XDG_RUNTIME_DIR;
      }
    });
  });

  describe('listen', () => {
    it('creates socket file', async () => {
      server.listen({ socketPath });
      await server.ready;
      expect(fs.existsSync(socketPath)).toBe(true);
    });

    it('sets correct socket permissions', async () => {
      server.listen({ socketPath });
      await server.ready;
      const stats = fs.statSync(socketPath);
      // Check owner-only permissions (0600)
      expect(stats.mode & 0o777).toBe(0o600);
    });

    it('removes stale socket file', async () => {
      // Create a stale socket file
      fs.writeFileSync(socketPath, '');

      // Server should remove it and start successfully
      server.listen({ socketPath });
      await server.ready;
      expect(fs.existsSync(socketPath)).toBe(true);
    });
  });

  describe('close', () => {
    it('removes socket file', async () => {
      server.listen({ socketPath });
      await server.ready;
      expect(fs.existsSync(socketPath)).toBe(true);

      server.close();
      expect(fs.existsSync(socketPath)).toBe(false);
    });

    it('disconnects all clients', async () => {
      server.listen({ socketPath });
      await server.ready;

      const client1 = await createClient();
      const client2 = await createClient();

      await new Promise((r) => setTimeout(r, 50));
      expect(server.getClientCount()).toBe(2);

      let client1Closed = false;
      let client2Closed = false;
      client1.on('close', () => (client1Closed = true));
      client2.on('close', () => (client2Closed = true));

      server.close();

      await new Promise((r) => setTimeout(r, 50));
      expect(client1Closed).toBe(true);
      expect(client2Closed).toBe(true);
    });
  });

  describe('client connections', () => {
    it('emits client_connected on new connection', async () => {
      server.listen({ socketPath });
      await server.ready;

      const connectedHandler = mock((_clientId: string) => {});
      server.on('client_connected', connectedHandler);

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      expect(connectedHandler).toHaveBeenCalled();
      expect(server.getClientCount()).toBe(1);

      client.destroy();
    });

    it('emits client_disconnected when client closes', async () => {
      server.listen({ socketPath });
      await server.ready;

      const disconnectedHandler = mock((_clientId: string) => {});
      server.on('client_disconnected', disconnectedHandler);

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      client.destroy();
      await new Promise((r) => setTimeout(r, 50));

      expect(disconnectedHandler).toHaveBeenCalled();
      expect(server.getClientCount()).toBe(0);
    });

    it('handles multiple clients', async () => {
      server.listen({ socketPath });
      await server.ready;

      const client1 = await createClient();
      const client2 = await createClient();
      const client3 = await createClient();

      await new Promise((r) => setTimeout(r, 50));

      expect(server.getClientCount()).toBe(3);
      expect(server.getClientIds().length).toBe(3);

      client1.destroy();
      client2.destroy();
      client3.destroy();
    });

    it('assigns unique client IDs', async () => {
      server.listen({ socketPath });
      await server.ready;

      const clientIds: string[] = [];
      server.on('client_connected', (id) => clientIds.push(id));

      const client1 = await createClient();
      const client2 = await createClient();

      await new Promise((r) => setTimeout(r, 50));

      expect(clientIds.length).toBe(2);
      expect(clientIds[0]).not.toBe(clientIds[1]);

      client1.destroy();
      client2.destroy();
    });
  });

  describe('message handling', () => {
    it('parses valid JSONL messages', async () => {
      server.listen({ socketPath });
      await server.ready;

      const messageHandler = mock((_clientId: string, _msg: ClientMessage) => {});
      server.on('client_message', messageHandler);

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      sendMessage(client, { type: 'start' });
      await new Promise((r) => setTimeout(r, 50));

      expect(messageHandler).toHaveBeenCalled();

      client.destroy();
    });

    it('handles initialize message', async () => {
      server.listen({ socketPath });
      await server.ready;

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      sendMessage(client, { type: 'initialize', version: '2.0.0' });

      const response = await waitForMessage(client);

      expect(response.type).toBe('initialized');
      if (response.type === 'initialized') {
        expect(response.client_id).toMatch(/^client_\d+$/);
        expect(response.daemon_version).toBeDefined();
      }

      client.destroy();
    });

    it('sends error for invalid messages', async () => {
      server.listen({ socketPath });
      await server.ready;

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      sendMessage(client, { type: 'invalid_type' });

      const response = await waitForMessage(client);

      expect(response.type).toBe('error');
      if (response.type === 'error') {
        expect(response.code).toBe('INTERNAL_ERROR');
        expect(response.recoverable).toBe(true);
      }

      client.destroy();
    });

    it('handles multiple messages in one data chunk', async () => {
      server.listen({ socketPath });
      await server.ready;

      const messages: ClientMessage[] = [];
      server.on('client_message', (_id, msg) => messages.push(msg));

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      // Send multiple messages in one write
      client.write('{"type":"start"}\n{"type":"stop"}\n');
      await new Promise((r) => setTimeout(r, 50));

      expect(messages.length).toBe(2);
      expect(messages[0].type).toBe('start');
      expect(messages[1].type).toBe('stop');

      client.destroy();
    });

    it('handles split messages across data chunks', async () => {
      server.listen({ socketPath });
      await server.ready;

      const messages: ClientMessage[] = [];
      server.on('client_message', (_id, msg) => messages.push(msg));

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      // Split a message across two writes
      client.write('{"type":');
      await new Promise((r) => setTimeout(r, 10));
      client.write('"start"}\n');
      await new Promise((r) => setTimeout(r, 50));

      expect(messages.length).toBe(1);
      expect(messages[0].type).toBe('start');

      client.destroy();
    });
  });

  describe('send/broadcast', () => {
    it('sends message to specific client', async () => {
      server.listen({ socketPath });
      await server.ready;

      let targetClientId = '';
      server.on('client_connected', (id) => (targetClientId = id));

      const client = await createClient();
      await new Promise((r) => setTimeout(r, 50));

      server.send(targetClientId, {
        type: 'status',
        state: 'idle',
        audio_ok: false,
        ws_ok: false,
      });

      const response = await waitForMessage(client);

      expect(response.type).toBe('status');

      client.destroy();
    });

    it('broadcasts message to all clients', async () => {
      server.listen({ socketPath });
      await server.ready;

      const client1 = await createClient();
      const client2 = await createClient();

      await new Promise((r) => setTimeout(r, 50));

      const message1Promise = waitForMessage(client1);
      const message2Promise = waitForMessage(client2);

      server.broadcast({
        type: 'status',
        state: 'listening',
        audio_ok: true,
        ws_ok: true,
      });

      const [msg1, msg2] = await Promise.all([message1Promise, message2Promise]);

      expect(msg1.type).toBe('status');
      expect(msg2.type).toBe('status');

      client1.destroy();
      client2.destroy();
    });

    it('does not crash on send to invalid client', async () => {
      server.listen({ socketPath });
      await server.ready;

      // Should not throw
      server.send('invalid_client', {
        type: 'status',
        state: 'idle',
        audio_ok: false,
        ws_ok: false,
      });
    });
  });

  describe('factory function', () => {
    it('creates server instance', () => {
      const srv = createSocketServer();
      expect(srv).toBeInstanceOf(SocketServer);
    });
  });
});
