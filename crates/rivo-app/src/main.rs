//! Rivo — a desktop coding agent built in pure Rust with Rune.
//!
//! Inspired by Claude Code's tool loop and OpenCode's desktop shell. The agent core lives in
//! `rivo-agent` and knows nothing about the UI; this crate is only the native window.

mod demo;
mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rivo_agent::{
    AgentSettings, ChatInput, Event, EventKind, KILO_FREE_MODELS, Message, ModelConfig,
    PermissionMode, Preset, Protocol, ProviderConfig, Role, RunConfig, Session, Skill, context,
};
use rune::{AppSpec, Chrome, Color, Cx, El, KeyEvent, Spring, TextEdit, Tx, col, row};
use rune_kit::shell::{PaneMotion, PaneState, SidebarPeek, SidebarToggle, SplitView};
use rune_kit::{ComboboxState, ControlSize};

pub const PROJECTS: [(&str, f32); 4] = [("rivo", 22.0), ("rune", 250.0), ("site", 150.0), ("notes", 320.0)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Provider,
    Connection,
    Model,
    Skills,
    Tools,
    Permissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorTab {
    Details,
    Tools,
    Skills,
    Context,
}

pub struct Rivo {
    pub sessions: Vec<Session>,
    pub selected: String,
    pub open_tabs: Vec<String>,
    pub closed_projects: Vec<String>,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub draft: TextEdit,
    pub base_url: TextEdit,
    pub api_key: TextEdit,
    pub model_name: TextEdit,
    pub config: ProviderConfig,
    pub auto_submit: bool,
    // Settings screen
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub settings_select_open: Option<String>,
    pub theme_override: Option<bool>,
    pub settings_preset: Preset,
    pub free_index: usize,
    pub model_picker: ComboboxState,
    pub connection_status: String,
    pub testing: bool,
    // Agent configuration
    pub model_config: ModelConfig,
    pub agent_settings: AgentSettings,
    pub skills: Vec<Skill>,
    pub inspector_tab: InspectorTab,
    pub context_window_edit: TextEdit,
    pub max_output_edit: TextEdit,
    pub temperature_edit: TextEdit,
    pub system_prompt: TextEdit,
    pub context_used: usize,
    pub context_window: usize,
    pub compaction_notice: Option<usize>,
    probe_rx: Option<mpsc::Receiver<Result<String, String>>>,
    events: mpsc::Receiver<Event>,
    engine_tx: mpsc::Sender<Event>,
    cancels: HashMap<String, Arc<AtomicBool>>,
    turns: HashMap<String, u64>,
    turn: u64,
    next_id: u64,
}

impl Rivo {
    fn new() -> Self {
        let (engine_tx, events) = mpsc::channel();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let project = cwd.file_name().and_then(|s| s.to_str()).unwrap_or("workspace").to_string();
        let session = Session::new("s1", "New session", project, cwd.clone());
        // Explicit environment variables win over the saved file (handy for CI / one-off runs).
        let env_override = ["RIVO_PROVIDER", "RIVO_API_KEY", "RIVO_BASE_URL", "RIVO_MODEL"]
            .iter()
            .any(|k| std::env::var_os(k).is_some());
        let (config, model_config, agent_settings) = if env_override {
            (ProviderConfig::from_env(), ModelConfig::default(), AgentSettings::default())
        } else {
            load_config().unwrap_or_else(|| (ProviderConfig::from_env(), ModelConfig::default(), AgentSettings::default()))
        };
        let (base, key, model) = (config.base_url.clone(), config.api_key.clone(), config.model.clone());
        let free_index = KILO_FREE_MODELS.iter().position(|m| m.id == config.model).unwrap_or(0);
        let preset = config.preset;
        let skills = rivo_agent::skills::discover(&cwd);
        let window = model_config.context_window;
        let max_output = model_config.max_output;
        let temperature = model_config.temperature;
        let system_prompt_text = agent_settings.system_prompt.clone();
        Rivo {
            sessions: vec![session],
            selected: "s1".into(),
            open_tabs: vec!["s1".into()],
            closed_projects: Vec::new(),
            sidebar: PaneState::new(272.0).range(140.0, 560.0),
            inspector: PaneState::new(290.0).range(220.0, 460.0),
            draft: TextEdit::new(""),
            base_url: TextEdit::new(base),
            api_key: TextEdit::new(key),
            model_name: TextEdit::new(model),
            config,
            auto_submit: true,
            settings_open: false,
            settings_tab: SettingsTab::Provider,
            settings_select_open: None,
            theme_override: None,
            settings_preset: preset,
            free_index,
            model_picker: ComboboxState::default(),
            connection_status: String::new(),
            testing: false,
            model_config,
            agent_settings,
            skills,
            inspector_tab: InspectorTab::Details,
            context_window_edit: TextEdit::new(window.to_string()),
            max_output_edit: TextEdit::new(max_output.to_string()),
            temperature_edit: TextEdit::new(format!("{temperature}")),
            system_prompt: TextEdit::new(system_prompt_text),
            context_used: 0,
            context_window: window,
            compaction_notice: None,
            probe_rx: None,
            events,
            engine_tx,
            cancels: HashMap::new(),
            turns: HashMap::new(),
            turn: 0,
            next_id: 2,
        }
    }

    pub fn current(&self) -> &Session {
        self.sessions.iter().find(|s| s.id == self.selected).unwrap_or(&self.sessions[0])
    }

    pub fn busy(&self) -> bool {
        self.current().busy
    }

    fn index(&self) -> usize {
        let id = &self.selected;
        self.sessions.iter().position(|s| &s.id == id).unwrap_or(0)
    }

    pub fn select(&mut self, id: &str) {
        self.selected = id.to_string();
        if !self.open_tabs.iter().any(|t| t == id) {
            self.open_tabs.push(id.to_string());
        }
    }

    pub fn close_tab(&mut self, id: &str) {
        self.open_tabs.retain(|t| t != id);
        if self.selected == id {
            self.selected = self.open_tabs.first().cloned().unwrap_or_else(|| self.sessions[0].id.clone());
            if !self.open_tabs.contains(&self.selected) {
                self.open_tabs.push(self.selected.clone());
            }
        }
    }

    pub fn toggle_project(&mut self, project: &str) {
        if let Some(i) = self.closed_projects.iter().position(|p| p == project) {
            self.closed_projects.remove(i);
        } else {
            self.closed_projects.push(project.to_string());
        }
    }

    pub fn new_session(&mut self) {
        let id = format!("s{}", self.next_id);
        self.next_id += 1;
        let cwd = self.current().cwd.clone();
        let project = self.current().project.clone();
        self.sessions.insert(0, Session::new(id.clone(), "New session", project, cwd));
        self.selected = id.clone();
        self.open_tabs.insert(0, id);
    }

    pub fn close_session(&mut self, id: &str) {
        if self.sessions.len() == 1 {
            return;
        }
        if let Some(cancel) = self.cancels.get(id) {
            cancel.store(true, Ordering::Relaxed);
        }
        self.sessions.retain(|s| s.id != id);
        self.open_tabs.retain(|t| t != id);
        if self.selected == id {
            self.selected = self.open_tabs.first().cloned().unwrap_or_else(|| self.sessions[0].id.clone());
        }
    }

    /// The inspector's three text fields only drive the custom endpoint.
    fn sync_custom_fields(&mut self) {
        if self.config.preset == Preset::Custom {
            self.config.base_url = self.base_url.text.trim().to_owned();
            self.config.api_key = self.api_key.text.trim().to_owned();
            self.config.model = self.model_name.text.trim().to_owned();
        }
    }

    // ---- Settings screen ---------------------------------------------------------------

    pub fn open_settings(&mut self) {
        self.settings_preset = self.config.preset;
        self.free_index = KILO_FREE_MODELS.iter().position(|m| m.id == self.config.model).unwrap_or(0);
        self.base_url = TextEdit::new(self.config.base_url.clone());
        self.api_key = TextEdit::new(self.config.api_key.clone());
        self.model_name = TextEdit::new(self.config.model.clone());
        self.model_picker.close();
        self.context_window_edit = TextEdit::new(self.model_config.context_window.to_string());
        self.max_output_edit = TextEdit::new(self.model_config.max_output.to_string());
        self.temperature_edit = TextEdit::new(format!("{}", self.model_config.temperature));
        self.system_prompt = TextEdit::new(self.agent_settings.system_prompt.clone());
        self.connection_status.clear();
        self.settings_open = true;
    }

    pub fn choose_tab(&mut self, tab: SettingsTab) {
        self.settings_tab = tab;
        self.settings_select_open = None;
        self.connection_status.clear();
    }

    pub fn toggle_settings_select(&mut self, key: &str) {
        self.settings_select_open = if self.settings_select_open.as_deref() == Some(key) { None } else { Some(key.to_string()) };
    }

    pub fn close_settings_select(&mut self) {
        self.settings_select_open = None;
    }

    pub fn restore_defaults(&mut self) {
        self.model_config = ModelConfig::default();
        self.agent_settings = AgentSettings::default();
        self.theme_override = None;
        self.settings_select_open = None;
        self.sync_model_fields();
        self.connection_status = "Defaults restored — press Apply & save to keep them.".into();
    }

    pub fn set_protocol(&mut self, protocol: Protocol) {
        self.model_config = ModelConfig::for_protocol(protocol);
        self.max_output_edit = TextEdit::new(self.model_config.max_output.to_string());
        self.context_window_edit = TextEdit::new(self.model_config.context_window.to_string());
        self.config.base_url = self.model_config.base_hint.clone();
        self.base_url = TextEdit::new(self.config.base_url.clone());
        self.connection_status.clear();
    }

    pub fn set_permission(&mut self, mode: PermissionMode) {
        self.agent_settings.permission = mode;
    }

    pub fn toggle_skill(&mut self, name: &str) {
        let all: Vec<String> = rivo_agent::skills::discover(&self.current().cwd)
            .iter().map(|s| s.name.clone()).collect();
        toggle_name(&mut self.agent_settings.enabled_skills, name, &all);
        let enabled = self.agent_settings.enabled_skills.clone();
        self.skills = rivo_agent::skills::discover(&self.current().cwd)
            .into_iter()
            .filter(|s| enabled.is_empty() || enabled.contains(&s.name))
            .collect();
    }

    pub fn toggle_tool(&mut self, name: &str) {
        let all: Vec<String> = rivo_agent::tools::all_tools().iter().map(|t| t.name().to_string()).collect();
        toggle_name(&mut self.agent_settings.enabled_tools, name, &all);
    }

    fn sync_model_fields(&mut self) {
        self.model_config.context_window = self.context_window_edit.text.trim().parse().unwrap_or(128_000).max(4_096);
        self.model_config.max_output = self.max_output_edit.text.trim().parse().unwrap_or(4096).max(256);
        let temp: f32 = self.temperature_edit.text.trim().parse().unwrap_or(0.2);
        self.model_config.temperature = temp.clamp(0.0, 2.0);
        self.agent_settings.system_prompt = self.system_prompt.text.clone();
    }

    pub fn close_settings(&mut self) {
        self.settings_open = false;
    }

    pub fn choose_preset(&mut self, preset: Preset) {
        self.settings_preset = preset;
        self.connection_status.clear();
    }

    pub fn choose_free_model(&mut self, index: usize) {
        self.free_index = index;
        self.connection_status.clear();
    }

    /// Switch the active model from the composer picker. Picking a Kilo free model also
    /// switches the provider to the free gateway, so the composer is enough for a quick change.
    pub fn choose_model(&mut self, id: &str) {
        if let Some(index) = KILO_FREE_MODELS.iter().position(|m| m.id == id) {
            let model = KILO_FREE_MODELS[index];
            let used = context::conversation_tokens(&self.current().messages);
            let window = parse_context_label(model.context).unwrap_or(self.context_window);
            self.free_index = index;
            self.config = ProviderConfig::kilo_free(model.id);
            self.settings_preset = Preset::KiloFree;
            self.model_name = TextEdit::new(self.config.model.clone());
            self.base_url = TextEdit::new(self.config.base_url.clone());
            self.api_key = TextEdit::new(self.config.api_key.clone());
            self.model_config.context_window = window;
            self.context_window = window;
            self.context_window_edit = TextEdit::new(window.to_string());
            self.context_used = used;
            self.connection_status.clear();
            self.model_picker.close();
            save_config(&self.config, &self.model_config, &self.agent_settings);
        }
    }

    /// The config the settings form currently describes, before pressing Apply.
    fn pending_config(&self) -> ProviderConfig {
        match self.settings_preset {
            Preset::Mock => ProviderConfig::mock(),
            Preset::KiloFree => ProviderConfig::kilo_free(KILO_FREE_MODELS[self.free_index].id),
            Preset::Custom => ProviderConfig::custom(
                self.base_url.text.trim().to_owned(),
                self.api_key.text.trim().to_owned(),
                self.model_name.text.trim().to_owned(),
            ),
        }
    }

    pub fn apply_settings(&mut self) {
        self.sync_model_fields();
        self.config = self.pending_config();
        let (base, key, model) = (self.config.base_url.clone(), self.config.api_key.clone(), self.config.model.clone());
        self.base_url = TextEdit::new(base);
        self.api_key = TextEdit::new(key);
        self.model_name = TextEdit::new(model);
        self.context_window = self.model_config.context_window;
        save_config(&self.config, &self.model_config, &self.agent_settings);
        self.connection_status = "Saved to ~/.config/rivo/config.json".into();
    }

    pub fn test_connection(&mut self) {
        self.sync_model_fields();
        let config = self.pending_config();
        let protocol = self.model_config.protocol;
        if let Some(problem) = config.problem() {
            self.connection_status = problem.into();
            return;
        }
        self.testing = true;
        self.connection_status = "Testing connection…".into();
        let (tx, rx) = mpsc::channel();
        self.probe_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(rivo_agent::provider::probe(&config, protocol));
        });
    }

    pub fn submit(&mut self) {
        let prompt = self.draft.text.trim().to_owned();
        if prompt.is_empty() || self.current().busy {
            return;
        }
        self.sync_custom_fields();
        self.draft = TextEdit::new("");

        let idx = self.index();
        let id = self.sessions[idx].id.clone();
        self.turn += 1;
        let turn = self.turn;
        self.turns.insert(id.clone(), turn);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.insert(id.clone(), cancel.clone());

        {
            let session = &mut self.sessions[idx];
            if session.messages.len() == 1 {
                session.title = title_from(&prompt);
            }
            session.push(Message::new(Role::User, prompt));
            session.push(Message::new(Role::Assistant, ""));
            session.busy = true;
        }

        let session = &self.sessions[idx];
        let count = session.messages.len();
        let input = ChatInput {
            messages: session.messages[..count - 1].to_vec(),
            cwd: session.cwd.clone(),
        };
        let run = RunConfig::new(
            self.config.clone(),
            self.model_config.clone(),
            self.agent_settings.clone(),
            &session.cwd,
        );
        self.context_used = context::conversation_tokens(&input.messages);
        let _ = rivo_agent::engine::spawn(id, turn, cancel, input, run, self.engine_tx.clone());
    }

    pub fn stop(&mut self) {
        let idx = self.index();
        let id = self.sessions[idx].id.clone();
        if let Some(cancel) = self.cancels.get(&id) {
            cancel.store(true, Ordering::Relaxed);
        }
        let t = self.turns.entry(id).or_insert(0);
        *t += 1;
        self.sessions[idx].busy = false;
    }

    fn apply_event(&mut self, ev: Event) -> bool {
        if self.turns.get(&ev.session).copied() != Some(ev.turn) {
            return false;
        }
        let idx = match self.sessions.iter().position(|s| s.id == ev.session) {
            Some(i) => i,
            None => return false,
        };
        let session = &mut self.sessions[idx];
        match ev.kind {
            EventKind::Delta(text) => {
                if let Some(last) = session.last_assistant_mut() {
                    last.text.push_str(&text);
                }
            }
            EventKind::ToolStarted(call) => {
                if let Some(last) = session.last_assistant_mut() {
                    last.tools.push(call);
                }
            }
            EventKind::ToolFinished { id, result, ok } => {
                if let Some(last) = session.last_assistant_mut() {
                    if let Some(call) = last.tools.iter_mut().find(|c| c.id == id) {
                        call.finished = true;
                        call.ok = ok;
                        call.result = Some(result);
                    }
                }
            }
            EventKind::Done => {
                session.busy = false;
            }
            EventKind::Error(message) => {
                if let Some(last) = session.last_assistant_mut() {
                    if !last.text.is_empty() {
                        last.text.push_str("\n\n");
                    }
                    last.text.push_str(&format!("**Error:** {message}"));
                }
                session.busy = false;
            }
            EventKind::ContextCompacted { removed } => {
                self.compaction_notice = Some(removed);
            }
            EventKind::Usage { used, window } => {
                self.context_used = used;
                self.context_window = window;
            }
        }
        true
    }
}

/// Settings live next to Kilo's own config, in `~/.config/rivo/config.json`.
fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"))
        .join("rivo")
        .join("config.json")
}

type SavedConfig = (ProviderConfig, ModelConfig, AgentSettings);

fn load_config() -> Option<SavedConfig> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let get = |k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let preset = match v.get("preset").and_then(serde_json::Value::as_str).unwrap_or("") {
        "mock" => Preset::Mock,
        "kilo" => Preset::KiloFree,
        "custom" => Preset::Custom,
        _ => return None,
    };
    let provider = match preset {
        Preset::Mock => ProviderConfig::mock(),
        Preset::KiloFree => ProviderConfig::kilo_free(&get("model")),
        Preset::Custom => ProviderConfig::custom(get("base_url"), get("api_key"), get("model")),
    };
    let protocol = match v.get("protocol").and_then(serde_json::Value::as_str).unwrap_or("") {
        "anthropic" => Protocol::Anthropic,
        "gemini" => Protocol::Gemini,
        _ => Protocol::OpenAiCompatible,
    };
    let mut model = ModelConfig::for_protocol(protocol);
    if let Some(n) = v.get("context_window").and_then(serde_json::Value::as_u64) { model.context_window = n as usize; }
    if let Some(n) = v.get("max_output").and_then(serde_json::Value::as_u64) { model.max_output = n as usize; }
    if let Some(n) = v.get("temperature").and_then(serde_json::Value::as_f64) { model.temperature = n as f32; }
    let mut settings = AgentSettings::default();
    settings.permission = match v.get("permission").and_then(serde_json::Value::as_str).unwrap_or("") {
        "readonly" => PermissionMode::ReadOnly,
        _ => PermissionMode::Auto,
    };
    settings.system_prompt = get("system_prompt");
    settings.max_steps = v.get("max_steps").and_then(serde_json::Value::as_u64).unwrap_or(12) as usize;
    settings.auto_compact = v.get("auto_compact").and_then(serde_json::Value::as_bool).unwrap_or(true);
    settings.keep_recent = v.get("keep_recent").and_then(serde_json::Value::as_u64).unwrap_or(8) as usize;
    let string_list = |k: &str| -> Vec<String> {
        v.get(k).and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    settings.enabled_tools = string_list("enabled_tools");
    settings.enabled_skills = string_list("enabled_skills");
    Some((provider, model, settings))
}

fn save_config(config: &ProviderConfig, model: &ModelConfig, settings: &AgentSettings) {
    let preset = match config.preset {
        Preset::Mock => "mock",
        Preset::KiloFree => "kilo",
        Preset::Custom => "custom",
    };
    let protocol = match model.protocol {
        Protocol::OpenAiCompatible => "openai",
        Protocol::Anthropic => "anthropic",
        Protocol::Gemini => "gemini",
    };
    let permission = match settings.permission {
        PermissionMode::Auto => "auto",
        PermissionMode::ReadOnly => "readonly",
    };
    let json = serde_json::json!({
        "preset": preset,
        "base_url": config.base_url,
        "api_key": config.api_key,
        "model": config.model,
        "protocol": protocol,
        "context_window": model.context_window,
        "max_output": model.max_output,
        "temperature": model.temperature,
        "permission": permission,
        "system_prompt": settings.system_prompt,
        "max_steps": settings.max_steps,
        "auto_compact": settings.auto_compact,
        "keep_recent": settings.keep_recent,
        "enabled_tools": settings.enabled_tools,
        "enabled_skills": settings.enabled_skills,
    });
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
        eprintln!("rivo: could not save settings to {}: {e}", path.display());
    }
}

/// `enabled_tools`/`enabled_skills` are empty when everything is on. Toggling one item in that
/// state builds an explicit list of all the *other* names, then edits that list.
fn toggle_name(list: &mut Vec<String>, name: &str, all: &[String]) {
    if list.is_empty() {
        *list = all.iter().filter(|n| n.as_str() != name).cloned().collect();
    } else if let Some(i) = list.iter().position(|n| n == name) {
        list.remove(i);
        if list.is_empty() {
            list.extend_from_slice(all);
        }
    } else {
        list.push(name.to_string());
        if list.len() == all.len() {
            list.clear();
        }
    }
}

fn title_from(prompt: &str) -> String {
    let first = prompt.lines().next().unwrap_or(prompt).trim();
    let mut title: String = first.chars().take(46).collect();
    if first.chars().count() > 46 {
        title.push('…');
    }
    if title.is_empty() { "New session".into() } else { title }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn poll(s: &mut Rivo, _clock: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    while let Ok(ev) = s.events.try_recv() {
        changed |= s.apply_event(ev);
    }
    if let Some(rx) = &s.probe_rx {
        if let Ok(result) = rx.try_recv() {
            s.connection_status = match result {
                Ok(message) => message,
                Err(message) => format!("Connection failed: {message}"),
            };
            s.testing = false;
            s.probe_rx = None;
            changed = true;
        }
    }
    if s.busy() || s.testing {
        tx.wake_after(ms(16));
    }
    changed
}

fn on_key(s: &mut Rivo, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key == rune::Key::Escape && s.settings_open {
        s.close_settings();
        return true;
    }
    if rune_kit::shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('n') && ev.mods.shift {
        s.new_session();
        return true;
    }
    if ev.is_command('i') && ev.mods.alt {
        s.inspector.toggle();
        return true;
    }
    false
}

fn parse_context_label(label: &str) -> Option<usize> {
    let label = label.trim();
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let value: usize = digits.parse().ok()?;
    let suffix = label[digits.len()..].trim();
    let multiplier = if suffix.is_empty() {
        1
    } else if suffix.eq_ignore_ascii_case("k") {
        1_000
    } else if suffix.eq_ignore_ascii_case("m") {
        1_000_000
    } else {
        return None;
    };
    value.checked_mul(multiplier)
}

fn view(s: &Rivo, cx: &mut Cx<Rivo>) -> El {
    let dark = s.theme_override.unwrap_or_else(|| cx.system_dark());
    let k = rune_theme_opencode::kit(dark);
    if s.settings_open {
        return views::settings_page(s, k, cx.titlebar_inset());
    }
    let inset = cx.titlebar_inset();
    let fresh = s.current().messages.len() <= 1 && !s.busy();
    let mut layer = Vec::new();
    let main = col()
        .key("main-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(if dark { Color::BLACK } else { Color::WHITE })
        .child(views::tab_bar(s, k, inset))
        .child(
            col().grow(1.0).min_h(0.0)
                .child(if fresh { views::empty_state(s, k) } else { views::transcript(s, k) }),
        )
        .child(row().w_full().padding(14.0).center().child(views::composer(s, k, &mut layer)));

    // The shell card (and the empty toolbar band above the transcript) must be black too;
    // panels keep the normal palette so they contrast against the centre.
    let mut shell_kit = k.clone();
    shell_kit.c.bg_base = if dark { Color::BLACK } else { Color::WHITE };
    let split = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut Rivo| &mut s.sidebar, views::sidebar(s, k))
        .inspector(&s.inspector, |s: &mut Rivo| &mut s.inspector, views::inspector(s, k))
        .titlebar_inset(inset)
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(14.0, 10.0)
        .sidebar_toggle(
            SidebarToggle::new()
                .size(ControlSize::Large)
                .past_lights(16.0)
                .edge(18.0)
                .gap(4.0)
                .in_header(),
        )
        .pane_motion(
            PaneMotion::new()
                .spring(Spring::new(0.3, 0.0))
                .scale_with_width(480.0, 0.15, 0.3)
                .fade_tail(0.15),
        )
        .sidebar_peek(SidebarPeek::new().edge(16.0, ms(40)).toggle_delay(ms(80)).below(56.0).hold(15.0, ms(100)))
        .reduced_motion(cx.is_reduced_motion())
        .build(&shell_kit);

    rune_kit::root(
        rune::row().w_full().h_full().child(views::rail(s, k, inset)).child(split.grow(1.0)),
        layer,
    )
}

fn scene_demo(s: &mut Rivo) {
    s.sessions = vec![demo::demo_session()];
    s.selected = "demo".into();
    s.open_tabs = vec!["demo".into()];
}

fn scene_streaming(s: &mut Rivo) {
    s.sessions = vec![demo::streaming_session()];
    s.selected = "demo".into();
    s.open_tabs = vec!["demo".into()];
}

fn scene_settings(s: &mut Rivo) {
    s.settings_open = true;
    s.settings_preset = Preset::KiloFree;
    s.free_index = 0;
    s.connection_status = "Ready to connect — no API key needed.".into();
}

fn main() {
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        scenes: &[("demo", scene_demo), ("streaming", scene_streaming), ("settings", scene_settings)],
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Rivo — native coding agent", (1160.0, 760.0), Rivo::new, view)
    };
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true };
    rune::App::new(spec).chrome(chrome).run();
}
