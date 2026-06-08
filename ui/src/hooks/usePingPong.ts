import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// The event name the Rust `ping` command emits (src-tauri/src/commands/ping.rs).
const PONG_EVENT = "pong";
// The Tauri command the frontend invokes to trigger that emission.
const PING_COMMAND = "ping";

interface UsePingPong {
  pong: string | null;
  sendPing: () => Promise<void>;
}

/**
 * M0 liveness wiring: subscribe to the backend's `pong` event and expose a
 * `sendPing` action that invokes the `ping` command. This proves the full
 * invoke -> command -> emit -> listen loop across the Tauri bridge with no
 * domain logic involved.
 */
export function usePingPong(): UsePingPong {
  const [pong, setPong] = useState<string | null>(null);

  useEffect(() => {
    // `listen` resolves to an unlisten function; clean it up on unmount.
    const unlistenPromise = listen<string>(PONG_EVENT, (event) => {
      setPong(event.payload);
      console.log(`[spectty] pong received: ${event.payload}`);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const sendPing = async (): Promise<void> => {
    await invoke(PING_COMMAND);
  };

  return { pong, sendPing };
}
