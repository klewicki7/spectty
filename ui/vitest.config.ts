import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Unit tests run in jsdom so React hooks/components have a DOM. The Tauri APIs
// (@tauri-apps/api/core + /event) are mocked per-test — no backend, no window.
export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: "jsdom",
    include: ["tests/**/*.test.{ts,tsx}"],
  },
});
