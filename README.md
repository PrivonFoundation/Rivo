<p align="center">
  <img src="assets/rivo-avatar.png" alt="Rivo" width="140" height="140" />
</p>

# Rivo

A **desktop coding agent in pure Rust**, built on [Rune](https://github.com/anomalyco/rune) (Anomaly's GPU-rendered native UI). No Electron, no webview, no TypeScript — one binary with its own renderer, text engine, layout and animations.

## Quick start

```bash
git clone --recurse-submodules https://github.com/PrivonFoundation/Rivo.git
cd Rivo
cargo run -p rivo-app --bin rivo
```

No API key? Rivo starts on a local mock provider that streams replies word by word and calls the real tools, so the whole UI can be exercised offline.

## Main features

- **50 tools** — files (read/write/edit/copy/move/delete, patch, diff), search (glob, grep, symbols), git (status/diff/log, branch, commit, stash, blame, …), commands (allowlisted + background tasks), web (fetch, search, download) and session (todos, memory, skills). Every call is shown in the transcript; toggle individual tools in the inspector.
- **3 wire protocols** — OpenAI-compatible, Anthropic Messages and Google Gemini.
- **models.dev catalog** — 324 providers and 10,024 models (1,036 free), searchable by name or category (All, Free, Tool-capable, Reasoning, Open weights). One click configures protocol, endpoint, model id, context window and max output. Cached for 24h at `~/.config/rivo/models-dev.json`, with a stale-cache fallback when offline.
- **Kilo Gateway** — kept as a preset with 14 tool-capable free models, no API key or card (200 requests/hour per IP).
- **MCP & Skills marketplaces** — add servers from the official MCP Registry and skills from skills.sh in one click, from Settings.
- **Info-centre inspector** — live context percentage, token/cost breakdown, per-model steps, MCP servers, code-indexing and LSP status, and changed files with `+/-` line counts.
- **Context management** — live token usage and automatic compaction of old turns into a summary marker.
- **Skills** — `SKILL.md` playbooks discovered from built-ins, `~/.config/rivo/skills/` and `<workspace>/.rivo/skills/`.
- **Sessions & tabs** — project-grouped sessions and multiple tabs.
- **Safety** — Auto / Read-only modes; workspace-confined paths and allowlisted commands; Stop cancels a running turn.

## Settings

The Settings window has seven tabs. All **model and context configuration lives in Providers** — there is no separate "Model & context" tab.

| Tab | What it does |
| --- | --- |
| General | Permissions, theme, workspace/skills folders, shortcut to Providers |
| Providers | Connection, protocol, models.dev catalog, Kilo Gateway, context and prompt settings |
| MCP Marketplace | Search the official MCP Registry and write entries to `~/.config/rivo/mcp.json` |
| Skills Marketplace | Search skills.sh and download skills into `<workspace>/.rivo/skills/` |
| Skills | Enable or disable discovered skills |
| Tools | Enable or disable individual agent tools |
| Permissions | Permission mode and safety information |

`Apply & save` writes `~/.config/rivo/config.json` (or `$XDG_CONFIG_HOME/rivo/config.json`). `Test connection` probes the `/models` endpoint from a worker thread. `Esc` returns to the session.

## Providers and models

Open **Settings → Providers**. The models.dev catalog is free, open source and needs no API key. When you click **Use**, Rivo fills in the endpoint, protocol, model id, context window and max output; it also picks up an API key from the environment variable named by the provider when one is set. Kilo Gateway remains available as a preset, including in the composer's quick model picker.

Environment variables override the saved config:

```bash
RIVO_PROVIDER=kilo RIVO_MODEL=cohere/north-mini-code:free cargo run -p rivo-app --bin rivo
```

```bash
export RIVO_PROVIDER=openai-compatible
export RIVO_BASE_URL=https://api.openai.com/v1
export RIVO_API_KEY=sk-...
export RIVO_MODEL=gpt-4o-mini
cargo run -p rivo-app --bin rivo
```

> Free tiers may log prompts for training. Don't send confidential code on them. Open-weight models need the same care: read the provider's terms and review a skill's `SKILL.md` before using it.

## Marketplaces and trust

- **MCP** comes from the [official MCP Registry](https://registry.modelcontextprotocol.io). Publishers verify a GitHub or DNS namespace, but the registry hosts metadata only and does **not** scan server code — review a server before adding it. Added servers load on restart.
- **Skills** come from [skills.sh](https://skills.sh), which shows install counts, the source repository and partner audit results. Audits are a signal, not a guarantee: read the skill before using it.
- Both searches and installs run on worker threads. Downloaded paths are confined to the skill folder, file counts and sizes are capped, and Rivo never executes downloaded code itself.

## Keyboard

| Shortcut | Action |
| --- | --- |
| `⌘/Ctrl + Shift + N` | New session |
| `⌘/Ctrl + Shift + B` | Toggle sidebar |
| `⌥/Alt + ⌘/Ctrl + I` | Toggle inspector |
| `Enter` in composer | Send message (`Shift+Enter` inserts a newline) |
| `Esc` in Settings | Back to the session |

## Headless verification

Rune renders headlessly with the same renderer as the window:

```bash
cargo run -p rivo-app --bin rivo -- --scene demo --snap /tmp/demo.png --size 1160x760
cargo run -p rivo-app --bin rivo -- --scene settings --snap /tmp/settings.png
cargo run -p rivo-app --bin rivo -- --scene providers --snap /tmp/providers.png
cargo run -p rivo-app --bin rivo -- --scene marketplace --snap /tmp/mcp.png
cargo run -p rivo-app --bin rivo -- --scene skill-marketplace --snap /tmp/skills.png
cargo run -p rivo-app --bin rivo -- --scene demo --tree
```

Run the test suite with `cargo test -p rivo-agent -p rivo-app`.

## Layout

```
crates/rivo-agent/   UI-agnostic engine: tools, protocols, catalog, context, skills, tool loop
crates/rivo-app/     Rune desktop UI: state, views, settings, headless scenes
vendor/rune/         pinned Rune checkout (Rune is pre-release)
```

The engine never imports Rune. It sends events over a channel; `AppSpec::poll` folds them into the view state, which keeps tool calls testable without a GPU. File-system and network work (git status, MCP/config discovery, registry and catalog downloads) runs on worker threads.
