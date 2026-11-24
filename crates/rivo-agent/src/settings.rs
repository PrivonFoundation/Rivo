//! Everything that shapes an agent turn but isn't a provider credential.

use crate::context::{ContextUsage, usage};
use crate::model::{Message, ProviderConfig, Protocol};
use crate::skills::{Skill, discover};
use std::path::Path;

/// What the agent is allowed to do during a turn. This is the agent-level
/// permission model surfaced in the UI and persisted in the config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMode {
    /// Read and research only. The agent investigates the workspace and
    /// produces a plan; it cannot modify files or run commands.
    Plan,
    /// Read, write, and run commands. Writes and commands stay confined to the
    /// workspace and commands are limited to a safe allowlist.
    Agent,
    /// Full permission. Reads, writes, and runs any command (no allowlist).
    /// File access still stays inside the workspace.
    Full,
}

impl AgentMode {
    pub fn label(self) -> &'static str {
        match self {
            AgentMode::Plan => "Plan (read-only)",
            AgentMode::Agent => "Agent",
            AgentMode::Full => "Full permission",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            AgentMode::Plan => "Reads and researches only; proposes a plan. No writes or commands.",
            AgentMode::Agent => "Reads, writes, and runs allowlisted commands in the workspace.",
            AgentMode::Full => "Reads, writes, and runs any command. No allowlist.",
        }
    }

    /// Parse a persisted/selected mode string, accepting legacy values.
    pub fn from_key(key: &str) -> Self {
        match key {
            "plan" | "readonly" => AgentMode::Plan,
            "full" => AgentMode::Full,
            _ => AgentMode::Agent,
        }
    }

    /// Stable key used when persisting to the config file.
    pub fn key(self) -> &'static str {
        match self {
            AgentMode::Plan => "plan",
            AgentMode::Agent => "agent",
            AgentMode::Full => "full",
        }
    }

    /// Whether file writes and edits are permitted.
    pub fn allows_write(self) -> bool {
        matches!(self, AgentMode::Agent | AgentMode::Full)
    }

    /// Whether commands may run at all.
    pub fn allows_command(self) -> bool {
        matches!(self, AgentMode::Agent | AgentMode::Full)
    }

    /// Whether the non-destructive command allowlist is enforced.
    pub fn enforces_command_allowlist(self) -> bool {
        matches!(self, AgentMode::Plan | AgentMode::Agent)
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
    pub mode: AgentMode,
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
            mode: AgentMode::Agent,
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
