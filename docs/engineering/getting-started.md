# Getting Started

Everything you need to clone, build, and run Spectty locally.

> ✅ M0 scaffolded the monorepo. The commands below are the **real, working** paths:
> clean clone → `pnpm install` → `pnpm tauri dev` → ping/pong in the web console.
> The target is < 30 min on a supported macOS with the prerequisites installed
> (most of that is the first clean Rust build of the Tauri/wry/webkit stack).

---

## Prerequisites

| Tool | Minimum version | How to check |
|---|---|---|
| Rust + Cargo | 1.89 | `rustc --version` |
| Node.js | 23 | `node --version` |
| pnpm | 9 | `pnpm --version` |
| Xcode Command Line Tools | any current | `xcode-select -p` |
| git | 2.x | `git --version` |

**macOS only (for now).** Linux support is planned; Windows is not on the roadmap.

### Install Xcode CLT (if missing)

```sh
xcode-select --install
```

### Install Rust (if missing)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install Node + pnpm (if missing)

```sh
# via fnm (recommended)
brew install fnm
fnm install 23
npm install -g pnpm@9
```

---

## The Tauri CLI

You do **not** need a global install. The Tauri CLI ships as a dev dependency
(`@tauri-apps/cli`) and is exposed through the root `tauri` pnpm script, so
`pnpm install` is all that's required. The canonical dev entry is `pnpm tauri dev`
(see below).

> A global `cargo install tauri-cli --locked` still works if you prefer `cargo tauri dev`,
> but the pnpm-scripted CLI is the supported, version-pinned path.

---

## Clone and install dependencies

```sh
git clone <repo-url> spectty
cd spectty
pnpm install          # installs UI deps + the pnpm-scoped Tauri CLI
```

Rust dependencies are fetched automatically on first build. The pinned toolchain
(`rust-toolchain.toml` → 1.89.0) is used regardless of your machine default.

---

## Run in development

```sh
pnpm tauri dev
```

This is the **canonical dev entry point**. It compiles the Rust backend, starts Vite
for the React UI on port 1420 (HMR), and opens the Tauri window.

**Verify the bridge:** click the ping button in the window and watch the web console
(right-click → Inspect Element → Console) — you should see `pong from spectty backend`,
proving the Tauri v2 invoke/emit wiring works end to end.

### Hot-reload split

- **UI (React/TypeScript):** Vite HMR reloads instantly on save — no rebuild needed.
- **Backend (Rust):** `pnpm tauri dev` rebuilds and restarts the backend on change.
  For a tighter Rust-only loop you can optionally run a watcher in a second terminal:

  ```sh
  cargo install cargo-watch   # one-time
  cargo watch -x 'build --workspace'
  ```

  `cargo-watch` is optional — it is not required to run the app.

---

## Production build

```sh
pnpm tauri build
```

The output is a `.app` bundle under `src-tauri/target/release/bundle/macos/`.

---

## Running tests

```sh
# Rust unit + integration tests
cargo test --workspace

# UI unit tests (Vitest)
pnpm --filter ui test

# Hexagonal boundary backstop — fails if spectty-core gains a forbidden dependency
cargo deny --manifest-path crates/core/Cargo.toml check bans
```

> E2E tests (Playwright / headless webview) are **deferred to M3+** and are not wired
> yet — there is no `pnpm test:e2e` in the M0 scaffold.

`cargo deny check` requires the `cargo-deny` binary:

```sh
cargo install cargo-deny --locked
```

See [Testing Strategy](testing-strategy.md) for the full test pyramid.

---

## Troubleshooting (macOS / Tauri / WebKit)

### `xcode-select: error: tool 'xcodebuild' requires Xcode`

You have CLT but not the full Xcode. Either install Xcode from the App Store, or
ensure Xcode CLT is active:

```sh
sudo xcode-select --switch /Library/Developer/CommandLineTools
```

### WebKit white screen on `pnpm tauri dev`

Vite's dev server starts on a port; the webview loads it via `localhost`. If you see
a blank window, check the Vite output for port conflicts. Kill whatever holds the port
and re-run.

### `dyld: Library not loaded` at launch

This usually means a native Rust dependency built against a different macOS SDK. Clean
and rebuild:

```sh
cargo clean
pnpm tauri dev
```

### Slow first build

Rust compiles all crates from scratch on first run (the Tauri/wry/webkit stack is the
bulk of it). Subsequent builds are incremental. `sccache` can speed up cold builds and
is enabled provisionally in CI:

```sh
brew install sccache
export RUSTC_WRAPPER=sccache
```

### macOS Gatekeeper blocking the `.app`

For local builds: right-click → Open in Finder, or:

```sh
xattr -d com.apple.quarantine path/to/Spectty.app
```

---

## What to read next

- [Project Structure](project-structure.md) — where everything lives
- [Conventions](conventions.md) — naming, commits, style
- [Testing Strategy](testing-strategy.md) — how we test
