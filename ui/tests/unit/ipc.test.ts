import { describe, expect, it, vi } from "vitest";

// `invoke` is the only Tauri surface the ipc wrappers touch directly. The
// Channel class is mocked so we never reach the real `window.__TAURI_INTERNALS__`.
const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  // Minimal Channel stand-in: records the handler assigned to `onmessage` so a
  // test can fire synthetic messages, mirroring how the real class dispatches.
  Channel: class {
    onmessage: ((message: unknown) => void) | null = null;
  },
}));

import {
  decodeChannelBytes,
  killPty,
  resizePty,
  sendInput,
  spawnPty,
} from "../../src/pty/ipc";

describe("decodeChannelBytes (R1: Channel<Vec<u8>> payload shape)", () => {
  it("decodes a JSON number[] (the shape Tauri v2 actually delivers)", () => {
    // A Rust `Vec<u8>` sent over `Channel<Vec<u8>>` arrives in JS as a JSON
    // number array because Tauri serializes it via serde_json, NOT as binary.
    const message = [104, 105]; // "hi"
    const bytes = decodeChannelBytes(message);

    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(bytes)).toEqual([104, 105]);
  });

  it("decodes an ArrayBuffer (the raw-Response fallback shape)", () => {
    const source = new Uint8Array([1, 2, 3]);
    const bytes = decodeChannelBytes(source.buffer);

    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(bytes)).toEqual([1, 2, 3]);
  });

  it("passes a Uint8Array through unchanged", () => {
    const source = new Uint8Array([7, 8, 9]);
    const bytes = decodeChannelBytes(source);

    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(bytes)).toEqual([7, 8, 9]);
  });
});

describe("pty ipc wrappers", () => {
  it("spawnPty invokes pty_spawn with cols/rows/cwd and the output channel", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue("pty-1");
    const channel = { onmessage: null };

    const id = await spawnPty(80, 24, "/tmp", channel as never);

    expect(id).toBe("pty-1");
    expect(invokeMock).toHaveBeenCalledWith("pty_spawn", {
      cols: 80,
      rows: 24,
      cwd: "/tmp",
      onOutput: channel,
    });
  });

  it("sendInput invokes send_input with id and a number[] payload", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);

    await sendInput("pty-1", new Uint8Array([97, 98]));

    expect(invokeMock).toHaveBeenCalledWith("send_input", {
      id: "pty-1",
      data: [97, 98],
    });
  });

  it("resizePty invokes pty_resize with id/cols/rows", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);

    await resizePty("pty-1", 120, 40);

    expect(invokeMock).toHaveBeenCalledWith("pty_resize", {
      id: "pty-1",
      cols: 120,
      rows: 40,
    });
  });

  it("killPty invokes pty_kill with id", async () => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);

    await killPty("pty-1");

    expect(invokeMock).toHaveBeenCalledWith("pty_kill", { id: "pty-1" });
  });
});
