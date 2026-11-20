<p align="center">
  <img src="assets/rivo-avatar.png" alt="Rivo" width="140" height="140" />
</p>

# Rivo

A **desktop coding agent in pure Rust**, built on [Rune](https://github.com/anomalyco/rune) (Anomaly's GPU-rendered native UI). No Electron, no webview, no TypeScript — one binary with its own renderer, text engine and animations.

## Quick start

```bash
git clone --recurse-submodules https://github.com/PrivonFoundation/Rivo.git
cd Rivo
cargo run -p rivo-app --bin rivo
```

No API key? Rivo starts on a local mock provider that streams replies and calls real tools, so the whole UI works offline.

## Main features

- **50 tools** — files (read/write/edit/copy/move/delete, patch, diff), search (glob, grep, symbols), git (status/diff/log, branch, commit, stash, blame, …), commands (allowlisted + background tasks), web (fetch, search, download) and session (todos, memory, skills). Every call is shown in the transcript; toggle individual tools in the inspector.
- **3 protocols** — OpenAI-compatible, Anthropic Messages and Google Gemini, switchable in **Settings → Providers**.
- **models.dev catalog** — 324 providers and 10k models (1k free), searchable by name or category (Free, Tool-capable, Reasoning, Open weights). One click configures protocol, endpoint, model id, context and output limits. Cached for 24h at `~/.config/rivo/models-dev.json`.
- **Kilo Gateway** — kept as a preset with 14 tool-capable free models, no API key or card (200 req/hour per IP).
- **MCP & Skills marketplaces** — find and add servers from the official MCP Registry and skills from skills.sh, right from Settings.
- **Context management** — live token usage and automatic compaction of old turns into a summary.
- **Skills** — `SKILL.md` playbooks discovered from built-ins, `~/.config/rivo/skills/` and `<workspace>/.rivo/skills/`.
- **Safety** — Auto / Read-only modes; workspace-confined paths and allowlisted commands; Stop cancels a running turn.
- **Sessions & tabs** — project-grouped sessions, multiple tabs and an info-centre inspector (context, token usage, models, MCP, git changes).

## Providers

All model, endpoint, protocol and context settings live in **Settings → Providers**. The models.dev catalog is free and needs no API key; an API key is read from the environment variable named by the provider when available. Kilo Gateway stays available as a preset. Environment variables still override the saved config:

```bash
RIVO_PROVIDER=kilo RIVO_MODEL=cohere/north-mini-code:free cargo run -p rivo-app --bin rivo
```

Rivo speaks OpenAI-style streaming, so it also works with OpenAI, OpenRouter, Groq, Ollama, LM Studio and similar endpoints:

```bash
export RIVO_PROVIDER=openai-compatible
export RIVO_BASE_URL=https://api.openai.com/v1
export RIVO_API_KEY=sk-...
export RIVO_MODEL=gpt-4o-mini
cargo run -p rivo-app --bin rivo
```

> Free tiers may log prompts for training. Don't send confidential code on them.

## Layout

```
crates/rivo-agent/   UI-agnostic engine: tools, protocols, context, skills, tool loop
crates/rivo-app/     Rune desktop UI: state, views, settings, headless scenes
vendor/rune/         pinned Rune checkout (Rune is pre-release)
```
