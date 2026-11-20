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

- **15 tools** — read/write/edit files, glob & search, git status/diff/log, allowlisted commands, web fetch, durable memory, skills and todos. Every call is shown in the transcript.
- **3 protocols** — OpenAI-compatible, Anthropic Messages and Google Gemini, switchable in Settings.
- **Free models** — Kilo Code gateway with 14 tool-capable free models, no API key or card (200 req/hour per IP).
- **MCP & Skills marketplaces** — find and add servers from the official MCP Registry and skills from skills.sh, right from Settings.
- **Context management** — live token usage and automatic compaction of old turns into a summary.
- **Skills** — `SKILL.md` playbooks discovered from built-ins, `~/.config/rivo/skills/` and `<workspace>/.rivo/skills/`.
- **Safety** — Auto / Read-only modes; workspace-confined paths and allowlisted commands; Stop cancels a running turn.
- **Sessions & tabs** — project-grouped sessions, multiple tabs and an inspector with Details / Tools / Skills / Context.

## Providers

Pick a provider in **Settings → Model**, or use environment variables (they override the saved config):

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
