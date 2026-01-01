# Fix: Multi-Client Text Insertion Error

## Problem

When multiple Neovim instances connect to the same dictate daemon (e.g., nested Neovim via sidekick.nvim → tmux → Claude Code → Ctrl+g), ALL clients receive transcription events and try to insert text. Non-initiating clients may have non-modifiable buffers, causing:

```
Error: failed to insert text: Buffer is not 'modifiable'
```

## Root Cause (Current Architecture)

The daemon broadcasts transcription events to all connected clients via `server.broadcast()`. Each Neovim client treats those events as "insert text into my current buffer/cursor position", so every connected client attempts insertion even if it didn't initiate dictation.

## Final Recommendation: Daemon-Enforced Session Ownership (Exclusive)

Implement an exclusive "session owner" in the daemon:

- The first client to send `start_listening` becomes the session owner.
- Only the owner receives transcription events (`speech_*`, `partial_transcript`, `final_transcript`).
- Status messages remain broadcast to all clients.
- Competing `start_listening` requests get a per-client `SESSION_BUSY` error.

Critical detail: ownership must persist through `flushing` and only be cleared when the daemon returns to `idle` (otherwise the final transcript can be dropped).

---

## Implementation Plan

### 1. Protocol Changes

**File**: `daemon/src/protocol.ts`

- Add `SESSION_BUSY` to `ErrorCodeSchema`.

### 2. Daemon Changes (Ownership + Routing)

**File**: `daemon/src/main.ts`

Add ownership tracking and helper(s):

```ts
let sessionOwner: string | null = null;

function sendToOwner(msg: DaemonMessage): void {
  if (sessionOwner) server.send(sessionOwner, msg);
}

function sendErrorToClient(
  clientId: string,
  code: ErrorCode,
  message: string,
  recoverable: boolean,
  hint?: string,
): void {
  server.send(clientId, { type: "error", code, message, recoverable, hint });
}
```

Update socket handlers to pass `clientId` into command handlers:

- `start_listening`:
  - If `sessionOwner === null`: claim ownership (`sessionOwner = clientId`) and start the normal listening sequence.
  - If `sessionOwner === clientId`: treat as idempotent.
  - If `sessionOwner !== clientId`: `sendErrorToClient(clientId, "SESSION_BUSY", ...)` and return.
- `stop_listening`:
  - If `clientId !== sessionOwner`: ignore (or optionally send a recoverable "not owner" error if desired).
  - If `clientId === sessionOwner`: run the existing stop/flush logic.
  - IMPORTANT: do not clear ownership here; keep it until the daemon reaches `idle`.
- On state transition `to === "idle"`:
  - Clear ownership (`sessionOwner = null`) and broadcast status.
- `client_disconnected`:
  - If disconnected client is the owner: force-stop the session (audio.stop + network.disconnect, clear pending transcripts) and clear ownership.
- Transcription events:
  - Replace broadcast of transcription events with `sendToOwner(...)`.
  - If `sessionOwner` is `null`, drop the event (no active session).
- Status broadcasts:
  - Keep broadcasting status to all clients.

### 3. Client Changes (Neovim)

**File**: `nvim/lua/dictate/job.lua`

Add local ownership tracking (no handshake needed):

```lua
local started_by_me = false
```

Update handlers:

- `M.start()`: Set `started_by_me = true` only after early returns, right before sending `start_listening`.
- `M.stop()`: Reset `started_by_me = false`.
- On `error` with code `SESSION_BUSY`: Reset `started_by_me = false`, show warning "Another Neovim instance is already dictating", optionally auto-stop dictatectl.

**File**: `nvim/lua/dictate/init.lua`

- Update `statusline()`: When state is `listening`/`flushing` and `started_by_me == false`, return "busy (other instance)" instead of the normal recording indicator.
- No transcript filtering needed because non-owners will not receive transcript events anymore.

### 4. Tests

**File**: `daemon/src/__tests__/integration/multi-client.test.ts`

Update existing tests that expect "transcription events broadcast to all clients" and add a new section (e.g. "5. Session ownership"):

- Only session owner receives transcription events
- Non-owner `start_listening` rejected with `SESSION_BUSY` (sent only to the requester)
- Ownership persists through `flushing` and is cleared on transition to `idle`
- Owner disconnect forces stop + clears ownership

### 5. Docs Parity

- Update `daemon/TEST_CHECKLIST.md` to reflect: status broadcasts to all clients, transcripts go only to the session owner.

---

## Edge Cases

| Scenario                           | Behavior                                                                                                |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Client A starts, A stops           | A owns session; receives transcripts through `flushing`. Ownership clears only on transition to `idle`. |
| Client A starts, A disconnects     | Daemon force-stops session, clears ownership, returns to `idle`.                                        |
| Client A starts, B tries to start  | B gets `SESSION_BUSY` (to B only), A continues uninterrupted.                                           |
| Client A starts, B tries to stop   | Ignored (or optional non-owner error), A continues.                                                     |
| New client connects during session | Receives status; uses local `started_by_me=false` to show "busy (other instance)".                      |

---

## Critical Files

- `daemon/src/protocol.ts` - Add `SESSION_BUSY` error code
- `daemon/src/main.ts` - Session ownership + per-client error + owner-only transcript routing
- `nvim/lua/dictate/job.lua` - Add `started_by_me` flag, handle `SESSION_BUSY` warning
- `nvim/lua/dictate/init.lua` - Update `statusline()` for "busy (other instance)" display
- `daemon/src/__tests__/integration/multi-client.test.ts` - Update broadcast assumptions + add ownership tests
