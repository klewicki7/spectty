//! `spectty-mcp` — stub MCP server for the Spectty Agent Protocol (Layer 1).
//!
//! M2 scaffold: the crate exists and is a registered workspace member so the
//! provisioner's `McpServerEntry.command` can point at a real binary path. The
//! stdio JSON-RPC handshake (`initialize` + `tools/list` + `tools/call` ack) is
//! implemented in WU-8 (PR4). This stub `main` is intentionally empty so
//! `cargo build --workspace` succeeds at PR1a without pulling the protocol in.
//!
//! Depends on serde/serde_json ONLY — NOT spectty-core, NOT tauri (D16).

fn main() {}
