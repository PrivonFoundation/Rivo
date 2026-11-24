# AGENTS.md

## What this is

Rivo is a native Rust desktop coding agent (Rune GPU UI, no Electron/webview/TypeScript).

- `crates/rivo-agent/` — UI-agnostic engine: tools, protocols, catalog, context, skills.
- `crates/rivo-app/` — Rune UI, binary name `rivo` (`src/main.rs`), plus headless scenes and the LAN remote bridge.
- `vendor/rune/` — pinned Rune submodule; the workspace `[patch.crates-io]` entries point into it.

## Setup and build gotchas

- Run `git submodule update --init --recursive` first. A fresh clone has an empty `vendor/rune`, and the workspace patches fail without it.
- Linux builds need system libraries (fontconfig, freetype, wayland, x11/xcb, GL/Vulkan, dbus, plus cmake/ninja/pkg-config). The full list is in `.github/workflows/release-linux.yml`.
- The interactive window needs a display/GPU. Headless scenes are the display-free way to render UI.
- `Cargo.lock` is committed and CI builds with `--locked`; commit lockfile changes when adding dependencies.

## Commands

- Build/run: `cargo build -p rivo-app --bin rivo` / `cargo run -p rivo-app --bin rivo`.
- Tests: `cargo test` from the root. Focused: `cargo test -p rivo-agent`, `cargo test -p rivo-app`; single test: `cargo test -p rivo-agent <test_name>`.
- Tests are offline and need no services. The live gateway test is ignored: `cargo test -p rivo-agent --test kilo_live -- --ignored --nocapture` (network, public free quota).
- No lint/format/typecheck config and no CI lint step; CI only builds the release binary.
- Headless scenes: `cargo run -p rivo-app --bin rivo -- --scene <name> --snap /tmp/out.png` (add `--tree` to dump the view tree). Scenes: `demo`, `checkpoints`, `streaming`, `settings`, `remote`, `providers`, `local`, `marketplace`, `skill-marketplace`.
- `.gitignore` excludes `*.png`, so write snapshots outside the repo (e.g. `/tmp`).

## Architecture rules

- `rivo-agent` must never import Rune. It runs turns on a worker thread and emits `Event`s over an `mpsc` channel; the UI folds events into view state. Keep that boundary.
- Blocking filesystem/network work stays on worker threads, not the UI thread.
- Tools live in `tools.rs` plus the private `tools_extra.rs` module.
- Headless scenes run before `AppSpec::setup`, so seed async data inside the scene function.
- Config: `~/.config/rivo/config.json`, overridable by `RIVO_PROVIDER`, `RIVO_API_KEY`, `RIVO_BASE_URL`, `RIVO_MODEL`. MCP servers: `~/.config/rivo/mcp.json`. Catalog cache: `~/.config/rivo/models-dev.json` (24h TTL); logos: `~/.config/rivo/logos/`.
- Skill precedence: built-ins, then `~/.config/rivo/skills`, then `<workspace>/.rivo/skills` (workspace wins on name collisions).

## Release

- Pushing any tag (or manual dispatch) builds Linux x86_64 and publishes a GitHub Release.
- Playbook: `.rivo/skills/rivo-release/SKILL.md` — run tests, render scenes, review diff. Do not bump versions or push without an explicit request.