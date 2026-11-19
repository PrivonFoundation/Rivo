//! The data model shared by the engine and the UI. Everything here is plain data:
//! the engine sends [`Event`]s over a channel and the UI folds them into its own state.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub ok: bool,
    pub finished: bool,
    pub approval: Option<String>,
}

impl ToolCall {
    pub fn new(id: String, name: String, arguments: String) -> Self {
        Self { id, name, arguments, result: None, ok: true, finished: false, approval: None }
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub tools: Vec<ToolCall>,
    pub ts: u64,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            tools: Vec::new(),
            ts: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub project: String,
    pub cwd: PathBuf,
    pub messages: Vec<Message>,
    pub busy: bool,
}

impl Session {
    pub fn new(id: impl Into<String>, title: impl Into<String>, project: impl Into<String>, cwd: PathBuf) -> Self {
        let title = title.into();
        let welcome = Message::new(
            Role::Assistant,
            format!(
                "I'm **Rivo**, your native coding agent. I can read and write files, search the workspace and run commands in `{}`.\n\nTell me what you want to build or fix. I'll show every tool call here so you can follow along.",
                cwd.display()
            ),
        );
        Self {
            id: id.into(),
            title,
            project: project.into(),
            cwd,
            messages: vec![welcome],
            busy: false,
        }
    }

    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    pub fn last_assistant_mut(&mut self) -> Option<&mut Message> {
        self.messages.iter_mut().rev().find(|m| m.role == Role::Assistant)
    }
}

/// A snapshot handed to the engine. The engine never touches UI state.
#[derive(Clone, Debug)]
pub struct ChatInput {
    pub messages: Vec<Message>,
    pub cwd: PathBuf,
}

/// Events emitted by the engine, drained by the UI in `AppSpec::poll`.
/// `turn` lets the UI ignore a stale worker after Stop or a new prompt.
#[derive(Clone, Debug)]
pub struct Event {
    pub session: String,
    pub turn: u64,
    pub kind: EventKind,
}

/// Token counters reported by a provider for one model call.
///
/// `input` holds only uncached input tokens: providers that bundle cached tokens into
/// their prompt count have them subtracted, so `cache_rate` stays comparable between
/// OpenAI, Anthropic and Gemini.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: TokenUsage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
    }

    /// Percentage of input served from cache, 0.0–100.0.
    pub fn cache_rate(&self) -> f64 {
        let total = self.input.saturating_add(self.cache_read);
        if total == 0 { 0.0 } else { self.cache_read as f64 / total as f64 * 100.0 }
    }
}

#[derive(Clone, Debug)]
pub enum EventKind {
    /// A chunk of assistant text (streaming).
    Delta(String),
    /// The model asked for a tool call.
    ToolStarted(ToolCall),
    /// A tool finished; `result` is truncated for display.
    ToolFinished { id: String, result: String, ok: bool },
    /// The current turn is complete.
    Done,
    /// Something failed.
    Error(String),
    /// Old turns were folded into a summary marker to fit the context window.
    ContextCompacted { removed: usize },
    /// Approximate context usage after a turn (input tokens).
    Usage { used: usize, window: usize },
    /// One provider call finished: a step of the agent's tool loop.
    ModelStep { model: String },
    /// Token counts reported by the provider for that call, when available.
    ModelUsage { model: String, usage: TokenUsage },
}

/// The Kilo Code free gateway: OpenAI-compatible, no API key, 200 requests/hour per IP.
pub const KILO_BASE_URL: &str = "https://api.kilo.ai/api/gateway";

/// One model from Kilo's free pool. Kept as data so the settings screen can list it.
#[derive(Clone, Copy, Debug)]
pub struct FreeModel {
    pub id: &'static str,
    pub name: &'static str,
    pub context: &'static str,
    pub note: &'static str,
}

/// The tool-capable free models in Kilo's live catalog (verified 2026-11-16).
/// The two `lyria` rows are omitted: they don't support tool calling.
pub const KILO_FREE_MODELS: &[FreeModel] = &[
    FreeModel { id: "kilo-auto/free", name: "Auto Free", context: "256K", note: "Rotates the free pool; prompts may be logged upstream." },
    FreeModel { id: "cohere/north-mini-code:free", name: "Cohere North Mini Code", context: "256K", note: "Tuned for code. A good default for this agent." },
    FreeModel { id: "nvidia/nemotron-3.5-lightning:free", name: "Nemotron 3.5 Lightning", context: "1M", note: "Fast, large context. Trial use only — don't send secrets." },
    FreeModel { id: "nvidia/nemotron-3.5-super-vl:free", name: "Nemotron 3.5 Super VL", context: "1M", note: "Multimodal. Trial use only — don't send secrets." },
    FreeModel { id: "nvidia/nemotron-3-ultra-550b-a55b:free", name: "Nemotron 3 Ultra 550B", context: "1M", note: "Large MoE model. Trial use only — don't send secrets." },
    FreeModel { id: "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free", name: "Nemotron 3 Nano Omni", context: "256K", note: "Reasoning model. Trial use only — don't send secrets." },
    FreeModel { id: "dots-studio/dots-3-note-preview:free", name: "Dots3 Note Preview", context: "512K", note: "Long output, text + vision." },
    FreeModel { id: "baidu/ernie-5.5-flash-preview:free", name: "ERNIE 5.5 Flash Preview", context: "1M", note: "Fast preview model." },
    FreeModel { id: "liquid/lfm-3-1.7b:free", name: "LiquidAI LFM3 1.7B", context: "256K", note: "Small and quick." },
    FreeModel { id: "particle-ai/particle-1:free", name: "Particle AI Particle-1", context: "1M", note: "Experimental." },
    FreeModel { id: "reflection/beam-free", name: "Reflection Beam", context: "1M", note: "Reasoning model; may emit long thinking traces." },
    FreeModel { id: "openrouter/free", name: "OpenRouter Free Router", context: "200K", note: "Routes across OpenRouter's free models." },
    FreeModel { id: "stealth/glyph-cluster", name: "Glyph Cluster", context: "256K", note: "Code-oriented model." },
    FreeModel { id: "humansand/ampersand-v0.1", name: "Ampersand v0.1", context: "1M", note: "Experimental." },
];

/// Configuration for an OpenAI-compatible chat endpoint.
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// Which preset the settings screen shows as selected.
    pub preset: Preset,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    /// Deterministic local demo: streams text and really runs tools.
    Mock,
    /// Any HTTP endpoint speaking one of the supported protocols.
    OpenAiCompatible,
}

/// Wire protocol used to talk to a remote endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Protocol::OpenAiCompatible => "OpenAI-compatible",
            Protocol::Anthropic => "Anthropic Messages",
            Protocol::Gemini => "Google Gemini",
        }
    }
    pub fn all() -> [Protocol; 3] {
        [Protocol::OpenAiCompatible, Protocol::Anthropic, Protocol::Gemini]
    }
}

// The old name is kept so existing callers/tests read naturally.
pub use Protocol as ChatProtocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Mock,
    KiloFree,
    Custom,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::Mock => "Mock provider",
            Preset::KiloFree => "Kilo Code — free models",
            Preset::Custom => "Custom / OpenAI-compatible",
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl ProviderConfig {
    pub fn from_env() -> Self {
        let api_key = std::env::var("RIVO_API_KEY").unwrap_or_default();
        let provider = std::env::var("RIVO_PROVIDER").ok();
        let base_url = std::env::var("RIVO_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let model = std::env::var("RIVO_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        if provider.as_deref() == Some("mock") || (provider.is_none() && api_key.is_empty() && base_url == "https://api.openai.com/v1") {
            return Self::mock();
        }
        if base_url.contains("api.kilo.ai") || provider.as_deref() == Some("kilo") {
            return Self::kilo_free(if model == "gpt-4o-mini" { "kilo-auto/free" } else { &model });
        }
        Self {
            kind: ProviderKind::OpenAiCompatible,
            preset: Preset::Custom,
            base_url,
            api_key,
            model,
        }
    }

    pub fn mock() -> Self {
        Self {
            kind: ProviderKind::Mock,
            preset: Preset::Mock,
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "mock".into(),
        }
    }

    pub fn kilo_free(model: &str) -> Self {
        Self {
            kind: ProviderKind::OpenAiCompatible,
            preset: Preset::KiloFree,
            base_url: KILO_BASE_URL.into(),
            api_key: String::new(), // the free gateway needs no key
            model: model.into(),
        }
    }

    pub fn custom(base_url: String, api_key: String, model: String) -> Self {
        Self { kind: ProviderKind::OpenAiCompatible, preset: Preset::Custom, base_url, api_key, model }
    }

    pub fn label(&self) -> &'static str {
        match self.preset {
            Preset::Mock => "mock (local)",
            Preset::KiloFree => "kilo code (free)",
            Preset::Custom => "openai-compatible",
        }
    }

    /// Validation used by the settings pane. The Kilo free gateway needs no API key.
    pub fn problem(&self) -> Option<&'static str> {
        match self.preset {
            Preset::Mock => None,
            Preset::KiloFree if self.model.is_empty() => Some("Pick a free model."),
            Preset::KiloFree => None,
            Preset::Custom if self.base_url.is_empty() => Some("Base URL is empty."),
            Preset::Custom if self.api_key.is_empty() => Some("API key is empty."),
            Preset::Custom if self.model.is_empty() => Some("Model is empty."),
            Preset::Custom => None,
        }
    }
}
