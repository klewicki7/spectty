import { Channel, invoke } from "@tauri-apps/api/core";

// The Tauri command names registered in `src-tauri/src/lib.rs` (PR2). Kept as
// constants so the camelCase JS args ↔ snake_case Rust args mapping lives in one
// place.
const PTY_SPAWN = "pty_spawn";
const SEND_INPUT = "send_input";
const PTY_RESIZE = "pty_resize";
const PTY_KILL = "pty_kill";

/**
 * The id the backend returns for a spawned PTY. `PtyId` is a `String` on the
 * Rust side (`src-tauri/src/pty_state.rs`), so it is a plain string here.
 */
export type PtyId = string;

/**
 * Decode whatever a `Channel<Vec<u8>>` message delivers into a `Uint8Array`.
 *
 * R1 (the long-flagged risk) RESOLVED: Tauri v2 serializes a Rust `Vec<u8>` sent
 * over `Channel::send` through `serde_json` (the blanket `impl<T: Serialize>
 * IpcResponse for T`), so it arrives in JS as a JSON **`number[]`**, NOT a
 * `Uint8Array`. Only an explicit `tauri::ipc::Response::new(..)` would yield an
 * `ArrayBuffer`. This helper handles all three shapes so it stays correct if the
 * backend ever switches to the raw-Response path:
 *   - `number[]`     → the actual M1 shape (PR2 sends a bare `Vec<u8>`)
 *   - `ArrayBuffer`  → the raw-Response fallback shape
 *   - `Uint8Array`   → already decoded, passed through
 */
export function decodeChannelBytes(message: unknown): Uint8Array {
  if (message instanceof Uint8Array) {
    return message;
  }
  if (message instanceof ArrayBuffer) {
    return new Uint8Array(message);
  }
  if (Array.isArray(message)) {
    return Uint8Array.from(message as number[]);
  }
  // ArrayBuffer views other than Uint8Array (defensive; not expected on this
  // path) — copy their underlying bytes.
  if (ArrayBuffer.isView(message)) {
    const view = message as ArrayBufferView;
    return new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
  }
  throw new TypeError(
    `unexpected pty channel payload shape: ${Object.prototype.toString.call(message)}`,
  );
}

/**
 * Build the `Channel<Uint8Array>` carrying decoded PTY output and bind its
 * `onmessage` to `onBytes`. The raw channel (returned for passing to
 * `spawnPty`) still receives the wire shape; `decodeChannelBytes` normalizes it
 * before `onBytes` ever sees it.
 */
export function createOutputChannel(
  onBytes: (bytes: Uint8Array) => void,
): Channel<unknown> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message: unknown) => {
    onBytes(decodeChannelBytes(message));
  };
  return channel;
}

/** Spawn a PTY of the given size and stream its output over `onOutput`. */
export async function spawnPty(
  cols: number,
  rows: number,
  cwd: string | undefined,
  onOutput: Channel<unknown>,
): Promise<PtyId> {
  return invoke<PtyId>(PTY_SPAWN, { cols, rows, cwd, onOutput });
}

/** Forward input bytes (a keystroke or paste) to a live PTY. */
export async function sendInput(id: PtyId, data: Uint8Array): Promise<void> {
  // Send a plain `number[]`: Tauri deserializes it straight into the Rust
  // `Vec<u8>` argument, with no dependency on special typed-array marshalling.
  await invoke(SEND_INPUT, { id, data: Array.from(data) });
}

/** Resize a live PTY (raises SIGWINCH for the child program). */
export async function resizePty(
  id: PtyId,
  cols: number,
  rows: number,
): Promise<void> {
  await invoke(PTY_RESIZE, { id, cols, rows });
}

/** Terminate a live PTY and clean up its read thread on the backend. */
export async function killPty(id: PtyId): Promise<void> {
  await invoke(PTY_KILL, { id });
}
