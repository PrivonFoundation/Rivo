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

- **53 tools** — files (read/write/edit/copy/move/delete, patch, diff), search (glob, grep, symbols), git (status/diff/log, branch, commit, stash, blame, …), commands (allowlisted + background tasks), web (fetch, search, download), session (todos, memory, skills) and **checkpoints**. Every call is shown in the transcript; toggle individual tools in the inspector.
- **3 wire protocols** — OpenAI-compatible, Anthropic Messages and Google Gemini.
- **models.dev catalog** — 324 providers and 10,024 models (1,036 free), searchable by name or category (All, Free, Tool-capable, Reasoning, Open weights). One click configures protocol, endpoint, model id, context window and max output. Cached at `~/.config/rivo/models-dev.json`: the cached copy is shown immediately, even when stale, while a bounded background refresh runs (10s). Official provider logos appear next to each model and are cached under `~/.config/rivo/logos/`.
- **Local model discovery** — scans the standard ports of Ollama, LM Studio, llama.cpp, vLLM, LocalAI, text-generation-webui, Jan, GPT4All, KoboldCpp, SGLang, llama-swap and Aphrodite. One click connects to a detected model; optionally scans the same Wi-Fi subnet and an extra host. No API key, no cloud.
- **Kilo Gateway** — kept as a preset with 14 tool-capable free models, no API key or card (200 requests/hour per IP).
- **MCP & Skills marketplaces** — add servers from the official MCP Registry and skills from skills.sh in one click, from Settings.
- **Info-centre inspector** — live context percentage, token/cost breakdown, per-model steps, MCP servers, code-indexing and LSP status, and changed files with `+/-` line counts.
- **Context management** — live token usage and automatic compaction of old turns into a summary marker.
- **Skills** — `SKILL.md` playbooks discovered from built-ins, `~/.config/rivo/skills/` and `<workspace>/.rivo/skills/`.
- **Checkpoints** — save a named snapshot of the workspace, list all checkpoints and restore any version from the inspector. Restoring first creates a `rivo-safety-*` tag, so it can be undone with one click. The agent can do the same through `checkpoint_create`, `checkpoint_list` and `checkpoint_restore`.
- **Sessions & tabs** — project-grouped sessions, multiple tabs and a right-hand inspector with **Info** and **Checkpoints** tabs (context, token usage, models, MCP, git changes, saved versions).
- **Safety** — Auto / Read-only modes; workspace-confined paths and allowlisted commands; Stop cancels a running turn.

## Settings

The Settings window has nine tabs. All **model and context configuration lives in Providers** — there is no separate "Model & context" tab.

| Tab | What it does |
| --- | --- |
| General | Permissions, theme, workspace/skills folders, shortcut to Providers |
| Providers | Connection, protocol, models.dev catalog, Kilo Gateway, context and prompt settings |
| Local models | Port scan, same-Wi-Fi discovery, detected models and one-click connect |
| Remote devices | LAN bridge for the phone app: enable, port, pairing code and paired devices |
| MCP Marketplace | Search the official MCP Registry and write entries to `~/.config/rivo/mcp.json` |
| Skills Marketplace | Search skills.sh and download skills into `<workspace>/.rivo/skills/` |
| Skills | Enable or disable discovered skills |
| Tools | Enable or disable individual agent tools |
| Permissions | Permission mode and safety information |

`Apply & save` writes `~/.config/rivo/config.json` (or `$XDG_CONFIG_HOME/rivo/config.json`). `Test connection` probes the `/models` endpoint from a worker thread. `Esc` returns to the session.

## Providers and models

Open **Settings → Providers**. The models.dev catalog is free, open source and needs no API key. When you click **Use**, Rivo fills in the endpoint, protocol, model id, context window and max output; it also picks up an API key from the environment variable named by the provider when one is set. Kilo Gateway remains available as a preset, including in the composer's quick model picker.

Each catalog row shows the **lab's** official logo from models.dev (`labs/<id>/logo.svg`), not the router that happens to serve the model — Claude gets Anthropic's mark, GPT gets OpenAI's, and a row reads `anthropic via AgentRouter` when the API provider differs from the lab. The lab comes from `canonical_model_id`; when that is missing, the provider id is used, with `providers/<id>/logo.svg` as a fallback. Rivo downloads missing logos in the background into `~/.config/rivo/logos/`; cached files (even past 24h) are reused so a slow network never delays the list. The upstream SVGs are monochrome (`currentColor`), so Rivo substitutes each lab's own brand colour before rasterizing — Anthropic Clay `#D97757`, Cohere coral `#FF7759`, DeepSeek `#4D6BFE`, NVIDIA green `#76B900`, OpenAI `#10A37F` — using the official white mark on dark for labs such as xAI and ElevenLabs. Labs without a verifiable brand colour get a light slate fallback, never black. Until a logo is cached, the row shows a neutral initial. No logo is fetched on the UI thread. models.dev does not publish per-model logos — only lab and provider ones.

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

## Local models

Open **Settings → Local models** (or **Providers → Find local models**). Rivo probes the standard ports of local runtimes on `127.0.0.1` on a worker thread, then asks each server that answers for its model list. Click **Use** on a model to set the endpoint, model id and a conservative context window — no API key or signup. The same models appear in the composer's model picker under **LOCAL MODELS**, so switching is one click from the chat too.

Supported runtimes and their default ports:

| Runtime | Port | Runtime | Port |
| --- | --- | --- | --- |
| Ollama | 11434 | LM Studio | 1234 |
| llama.cpp | 8080 | vLLM | 8000 |
| LocalAI | 8081 | text-generation-webui | 5000 |
| Jan | 1337 | GPT4All | 4891 |
| KoboldCpp | 5001 | SGLang | 30000 |
| llama-swap | 9297 | Aphrodite | 2242 |

The scan also covers common self-hosted ports (3000, 8888, 9000). Turn on **Same Wi-Fi** to also sweep this machine's `/24` subnet for servers on other devices, or type a single **Extra host** (IP or hostname). All servers are queried through their OpenAI-compatible `/v1/models` endpoint (with Ollama's native `/api/tags` as a fallback); only TCP connects and a small JSON model list leave Rivo. Local chats stay on the machine serving the model. Connect only to servers you trust.

For a fixed local endpoint, the `local` preset is also available in **Providers** and can be set from the environment:

```bash
RIVO_PROVIDER=local RIVO_BASE_URL=http://127.0.0.1:11434/v1 RIVO_MODEL=llama3.2 cargo run -p rivo-app --bin rivo
```

## Remote devices (phone bridge)

Enable the bridge in **Settings → Remote devices**. Rivo listens on the local network (default port 8787), shows the phone a URL (`http://<LAN-IP>:8787`) and a pairing code. The phone is a **full client**: it can list/create sessions, send messages (running the same agent tool loop), inspect results and manage checkpoints.

Pairing happens once with `POST /api/pair {"code": "...", "name": "Pixel"}`; the response contains the bearer token used by all later calls. The API:

```
GET  /api/status
GET  /api/sessions                 POST /api/sessions {"title": "..."}
GET  /api/sessions/{id}            POST /api/sessions/{id}/message {"text": "..."}
GET  /api/checkpoints              POST /api/checkpoints {"label": "..."}
POST /api/checkpoints/restore {"id": "..."}
```

Every call after pairing sends `Authorization: Bearer <token>`. The bridge is LAN-only: no cloud relay, no account, and it stops when Rivo closes. Anyone on the same network with the pairing code can read and change files in the workspace, so regenerate the code if a device is lost. Bridge settings are saved in `~/.config/rivo/config.json`.

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
cargo run -p rivo-app --bin rivo -- --scene checkpoints --snap /tmp/checkpoints.png
cargo run -p rivo-app --bin rivo -- --scene settings --snap /tmp/settings.png
cargo run -p rivo-app --bin rivo -- --scene providers --snap /tmp/providers.png
cargo run -p rivo-app --bin rivo -- --scene local --snap /tmp/local.png
cargo run -p rivo-app --bin rivo -- --scene remote --snap /tmp/remote.png
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
