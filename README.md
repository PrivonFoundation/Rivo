<p align="center">
  <img src="assets/rivo-avatar.png" alt="Rivo" width="120" />
</p>

# Rivo

A desktop coding agent in pure Rust, built on [Rune](https://github.com/anomalyco/rune). No Electron, no webview, no TypeScript — one binary with native GPU rendering, its own text engine and layout.

## Quick start

```bash
git clone --recurse-submodules https://github.com/PrivonFoundation/Rivo.git
cd Rivo
cargo run -p rivo-app --bin rivo
```

No API key needed to begin: Rivo starts on a local mock provider that streams replies and runs the real tools.

## Features

- **53 tools** — files, search, git, commands, web, sessions, checkpoints. Every call is shown in the transcript and can be toggled in the inspector.
- **Model sources** — models.dev catalog (324 providers, 10,024 models, 1,036 free), local model discovery, Kilo free gateway, custom OpenAI / Anthropic / Gemini endpoints.
- **Local models** — one click to connect to Ollama, LM Studio, llama.cpp and nine other runtimes on this machine or the same Wi-Fi. No key, no cloud.
- **Inspector** — live context, tokens, cost, per-model steps, MCP status, git changes and checkpoints.
- **Context & checkpoints** — automatic compaction of old turns; named workspace snapshots with one-click restore (a safety tag is created first).
- **Marketplaces** — MCP servers from the official registry and skills from skills.sh, installed from Settings.
- **Remote bridge** — pair a phone on the same LAN and use full sessions from the phone API.
- **Safety** — Auto / Read-only modes, workspace-confined paths, allowlisted commands, Stop to cancel a turn.

## Configuration

All model and context settings live in **Settings → Providers**: endpoint, protocol, model id, context window, max output and system prompt. `Apply & save` writes `~/.config/rivo/config.json`; environment variables override it.

```bash
RIVO_PROVIDER=kilo RIVO_MODEL=cohere/north-mini-code:free cargo run -p rivo-app --bin rivo

RIVO_PROVIDER=openai-compatible RIVO_BASE_URL=https://api.openai.com/v1 \
RIVO_API_KEY=sk-... RIVO_MODEL=gpt-4o-mini cargo run -p rivo-app --bin rivo
```

Catalog rows show each model's **lab logo in its official brand colour**, cached under `~/.config/rivo/logos/`. The catalog is cached at `~/.config/rivo/models-dev.json` and appears instantly while a bounded background refresh runs. Click **Use** to apply a model in one step.

> Free tiers may log prompts. Don't send confidential code to them; review a skill's `SKILL.md` before loading it.

## Local models

**Settings → Local models → Scan ports.** Rivo probes standard ports, reads `/v1/models` from each server and lists the models. Enable **Same Wi-Fi** to sweep the local `/24`, or enter an extra host. Detected models also appear in the composer picker.

Defaults: Ollama `11434`, LM Studio `1234`, llama.cpp `8080`, vLLM `8000`, LocalAI `8081`, text-generation-webui `5000`, Jan `1337`, GPT4All `4891`, KoboldCpp `5001`, SGLang `30000`, llama-swap `9297`, Aphrodite `2242`.

## Keyboard

| Shortcut | Action |
| --- | --- |
| `⌘/Ctrl + Shift + N` | New session |
| `⌘/Ctrl + Shift + B` | Toggle sidebar |
| `⌥/Alt + ⌘/Ctrl + I` | Toggle inspector |
| `Enter` / `Shift+Enter` | Send message / newline |
| `Esc` | Close Settings |

## Development

```bash
cargo test -p rivo-agent -p rivo-app
cargo run -p rivo-app --bin rivo -- --scene providers --snap /tmp/providers.png
```

Other headless scenes: `demo`, `checkpoints`, `settings`, `local`, `remote`, `marketplace`, `skill-marketplace`.

```
crates/rivo-agent/   UI-agnostic engine: tools, protocols, catalog, context, skills
crates/rivo-app/     Rune desktop UI: state, views, settings, headless scenes
vendor/rune/         pinned Rune checkout
```

The engine never imports Rune: it emits events over a channel, and the UI folds them into view state. Filesystem and network work runs on worker threads.
