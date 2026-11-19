<p align="center">
  <img src="assets/rivo-avatar.png" alt="Rivo" width="140" height="140" />
</p>

# Rivo

A **desktop coding agent in pure Rust**, built on [Rune](https://github.com/anomalyco/rune) —
the native GPU-rendered UI framework by Anomaly — and inspired by Claude Code's tool loop,
OpenCode's desktop shell and a friendly chat layout (icon rail, grouped sessions, tabs and a
floating composer). No Electron, no webview, no TypeScript: one Rust binary with its own renderer,
text engine, layout and spring animations.

## What makes it a complete agent

- **15 tools** — read/write/edit files, glob, grep-style search, git status/diff/log, allowlisted
  commands, web fetch, skills, todos and durable memory.
- **3 protocols** — OpenAI-compatible, Anthropic Messages and Google Gemini, switchable in
  Settings → Model with per-protocol base-URL hints.
- **Context management** — token estimate, live usage in the composer, auto-compaction of old
  turns into a summary marker, configurable window/max-output/temperature.
- **Skills** — `SKILL.md` playbooks with frontmatter, discovered from built-ins,
  `~/.config/rivo/skills/` and `<workspace>/.rivo/skills/`; shown as a catalog to the model and
  loadable with `skill_load`. An example lives in `.rivo/skills/rivo-release/SKILL.md`.
- **Permissions** — Auto or Read-only mode; tools are workspace-confined and commands allowlisted.
- **Sessions & tabs** — project-grouped session list, multiple tabs, per-project collapse.
- **Inspector tabs** — Details / Tools / Skills / Context, so the agent's state is always visible.

Clone with the pinned Rune checkout (it is a submodule, not vendored source):

```bash
git clone --recurse-submodules https://github.com/PrivonFoundation/Rivo.git
cd Rivo
cargo run -p rivo-app --bin rivo
```

With no API key set, Rivo starts on the **local mock provider**: it streams replies word by word
and still calls the real tools, so the whole UI can be exercised offline.

## Kilo Code free models (no API key)

Open **Settings** from the sidebar footer (or the inspector button) and pick
**Kilo Code — free**. The catalog lists the 14 tool-capable free models from Kilo's live
gateway, with a search filter, context size and a note for each. Recommended default:
**Cohere North Mini Code** (`cohere/north-mini-code:free`).

- Endpoint: `https://api.kilo.ai/api/gateway` — no credit card, no API key, 200 req/hour per IP.
- **Test connection** verifies `/models` live from a worker thread; the result appears at the bottom.
- **Apply & save** writes `~/.config/rivo/config.json`, so the choice survives restarts.
- Free-tier warning is shown in the UI: Auto Free and some providers may log prompts for
  training; NVIDIA endpoints are trial use only. Don't send confidential code on the free tier.

You can also select it from the environment:

```bash
RIVO_PROVIDER=kilo RIVO_MODEL=cohere/north-mini-code:free cargo run -p rivo-app --bin rivo
```

## Marketplaces (MCP & skills)

Settings has two extra tabs after **Model & context**, both free and without an API key:

- **MCP Marketplace** searches the [official MCP Registry](https://registry.modelcontextprotocol.io)
  (`GET /v0/servers?search=…`). Publishers verify a GitHub or DNS namespace. **Add** writes the
  server entry to `~/.config/rivo/mcp.json`; it is picked up after a restart. The registry hosts
  metadata only and does **not** scan server code — review a server before adding it.
- **Skills Marketplace** searches [skills.sh](https://skills.sh) (`GET /api/search?q=…`) and shows
  install counts plus the source repository. **Add** downloads the skill's text files into
  `<workspace>/.rivo/skills/`; the agent discovers it immediately. Read `SKILL.md` before using it.

Both searches and installs run on worker threads. Downloaded paths are confined to the skill
folder, file counts and sizes are capped, and Rivo never executes a downloaded script itself.

## Other providers

Rivo speaks the OpenAI chat-completions streaming protocol, so it works with OpenAI, OpenRouter,
Groq, Together, Ollama, LM Studio and others. Pick **Custom endpoint** in Settings, or use env
vars (which override the saved config when set):

```bash
export RIVO_PROVIDER=openai-compatible
export RIVO_BASE_URL=https://api.openai.com/v1     # or http://localhost:11434/v1
export RIVO_API_KEY=sk-...
export RIVO_MODEL=gpt-4o-mini
cargo run -p rivo-app --bin rivo
```

## Settings screen

A dedicated settings window with a left nav (General, Providers, Model & context, Skills, Tools,
Permissions), grouped cards with label/description rows and right-aligned Select/Switch controls,
a **Restore defaults** action and a fixed Apply & save / Test connection footer. Theme can follow
the system or be forced to Dark/Light. `Esc` or the back arrow returns to the session.
The right inspector pane also has quick Details/Tools/Skills/Context tabs.

## What it can do

The agent runs a real tool loop (up to 8 steps per turn). Every tool call is shown in the
transcript with its arguments and result.

| Tool | What it does |
| --- | --- |
| `list_dir`, `glob`, `search` | Tree listing, `**` glob matching, literal search with `file:line` |
| `read_file`, `write_file`, `edit_file` | Read, create and exact-unique-string replacement |
| `git_status`, `git_diff`, `git_log` | Working tree, unified diff and recent commits |
| `run_command` | Allowlisted read-only commands (`cargo`, `git`, `ls`, `rg`, `pwd`, …), 30 s timeout |
| `web_fetch`, `memory_write`, `memory_read` | http(s) text fetch and durable notes in `.rivo/memory.md` |
| `skill_load`, `todo_write` | Load a skill's full body; record a visible plan |

The tool loop works with real models too: `cargo test -p rivo-agent --test kilo_live -- --ignored`
runs one live turn through Kilo's free gateway (streamed reply + real `list_dir` call).

**Safety:** paths are resolved inside the session workspace and cannot escape it (absolute paths,
`..` and escaping symlinks are rejected). Shell metacharacters and non-allowlisted programs are
blocked. The engine runs on a worker thread; Stop cancels it and a turn id prevents stale events
from reaching the UI.

## Architecture

```
crates/
  rivo-agent/        UI-agnostic engine
    model.rs          sessions, messages, events, provider config, protocols
    tools.rs          15 tools + path/command sandboxing
    protocol.rs       OpenAI / Anthropic / Gemini streaming adapters
    provider.rs       provider dispatch, connection probe, local mock
    context.rs        token estimate, usage and compaction
    skills.rs         SKILL.md discovery and parsing
    settings.rs       model config, permissions, RunConfig snapshot
    engine.rs         the tool loop (emits events over mpsc)
  rivo-app/          Rune desktop UI
    main.rs           state, AppSpec::poll, window shell, settings persistence
    views.rs          rail, grouped sidebar, tabs, transcript, composer, inspector, settings
    demo.rs           seeded conversations for headless scenes
vendor/rune/          pinned Rune checkout (see Cargo.toml [patch.crates-io])
```

The engine never imports Rune. It sends `Event`s over a channel; `AppSpec::poll` folds them into
the state and Rune re-runs the view. That keeps tool calls unit-testable without a GPU.

## Headless verification

Rune renders headlessly with the same renderer as the window:

```bash
cargo run -p rivo-app --bin rivo -- --scene demo --snap /tmp/rivo-demo.png --size 1160x760
cargo run -p rivo-app --bin rivo -- --scene settings --snap /tmp/rivo-settings.png --size 1160x760
cargo run -p rivo-app --bin rivo -- --scene settings --script scripts/settings-tabs.json --at mark:skills --snap /tmp/skills.png
cargo run -p rivo-app --bin rivo -- --scene settings --script scripts/theme-light.json --at mark:light --snap /tmp/light.png
cargo run -p rivo-app --bin rivo -- --scene settings --script scripts/settings-test.json --tree
cargo run -p rivo-app --bin rivo -- --scene streaming --tree
```

## Keyboard

| Shortcut | Action |
| --- | --- |
| `⌘/Ctrl + N` | New session (with Shift in the demo binding) |
| `⇧⌘B` | Toggle sidebar (Rune shell) |
| `⌥⌘I` | Toggle inspector |
| `Esc` | Close settings |
| `Enter` in composer | Send message (`Shift+Enter` inserts a newline) |

## Notes

Rune is pre-release; the checkout in `vendor/rune` is pinned to
`55ef5268f22038a95344b3e9d2b4a25ddba731b1` (branch `dev`). Rune patches three published crates
(`winit`, `wgpu-hal`, `parley_engine`, plus AccessKit), and those patches are repeated in this
workspace's `Cargo.toml`, exactly as Rune's README requires for apps built outside its workspace.
