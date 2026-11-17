//! Everything that shapes an agent turn but isn't a provider credential.

use crate::context::{ContextUsage, usage};
use crate::model::{Message, ProviderConfig, Protocol};
use crate::skills::{Skill, discover};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    /// Tools run without asking (writes stay confined to the workspace).
    Auto,
    /// Writes and commands are refused; only read-only tools run.
    ReadOnly,
}

impl PermissionMode {
    pub fn label(self) -> &'static str {
        match self {
            PermissionMode::Auto => "Auto (tools allowed)",
            PermissionMode::ReadOnly => "Read-only",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub protocol: Protocol,
    /// Suggested base URL for this protocol, shown as a placeholder in settings.
    pub base_hint: String,
    pub context_window: usize,
    pub max_output: usize,
    pub temperature: f32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::OpenAiCompatible,
            base_hint: "https://api.openai.com/v1".into(),
            context_window: 128_000,
            max_output: 4096,
            temperature: 0.2,
        }
    }
}

impl ModelConfig {
    /// Sensible defaults when the user switches protocol in the settings screen.
    pub fn for_protocol(protocol: Protocol) -> Self {
        let mut m = Self::default();
        m.protocol = protocol;
        match protocol {
            Protocol::OpenAiCompatible => {}
            Protocol::Anthropic => {
                m.base_hint = "https://api.anthropic.com/v1".into();
                m.context_window = 200_000;
                m.max_output = 8192;
            }
            Protocol::Gemini => {
                m.base_hint = "https://generativelanguage.googleapis.com/v1beta".into();
                m.context_window = 1_000_000;
            }
        }
        m
    }

}

#[derive(Clone, Debug)]
pub struct AgentSettings {
    pub permission: PermissionMode,
    pub enabled_tools: Vec<String>,
    pub enabled_skills: Vec<String>,
    pub max_steps: usize,
    pub auto_compact: bool,
    pub keep_recent: usize,
    pub system_prompt: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            permission: PermissionMode::Auto,
            enabled_tools: Vec::new(), // empty means "all tools"
            enabled_skills: Vec::new(), // empty means "all skills"
            max_steps: 12,
            auto_compact: true,
            keep_recent: 8,
            system_prompt: String::new(),
        }
    }
}

impl AgentSettings {
    pub fn tool_enabled(&self, name: &str) -> bool {
        self.enabled_tools.is_empty() || self.enabled_tools.iter().any(|n| n == name)
    }
    pub fn skill_enabled(&self, name: &str) -> bool {
        self.enabled_skills.is_empty() || self.enabled_skills.iter().any(|n| n == name)
    }
}

/// One immutable snapshot handed to the engine thread.
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub provider: ProviderConfig,
    pub model: ModelConfig,
    pub settings: AgentSettings,
    pub skills: Vec<Skill>,
}

impl RunConfig {
    pub fn new(provider: ProviderConfig, model: ModelConfig, settings: AgentSettings, workspace: &Path) -> Self {
        let skills: Vec<Skill> = discover(workspace)
            .into_iter()
            .filter(|s| settings.skill_enabled(&s.name))
            .collect();
        Self { provider, model, settings, skills }
    }

    pub fn context(&self, messages: &[Message]) -> ContextUsage {
        usage(messages, self.model.context_window)
    }
}
