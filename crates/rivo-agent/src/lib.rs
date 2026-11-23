//! Rivo agent core: an OpenAI/Anthropic/Gemini coding-agent runtime with tools, skills,
//! context compaction and permissions. It never imports Rune: the UI folds `Event`s into state.

pub mod catalog;
pub mod checkpoint;
pub mod context;
pub mod engine;
pub mod info;
pub mod local;
pub mod model;
pub mod protocol;
pub mod provider;
pub mod registry;
pub mod settings;
pub mod skills;
pub mod tools;
mod tools_extra;

pub use local::{LocalModel, ScanReport};
pub use model::{
    ChatInput, Event, EventKind, FreeModel, Message, Preset, Protocol, ProviderConfig, ProviderKind,
    Role, Session, TokenUsage, ToolCall, KILO_BASE_URL, KILO_FREE_MODELS,
};
pub use settings::{AgentSettings, ModelConfig, PermissionMode, RunConfig};
pub use skills::Skill;
