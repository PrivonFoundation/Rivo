//! Rivo — a desktop coding agent built in pure Rust with Rune.
//!
//! Inspired by Claude Code's tool loop and OpenCode's desktop shell. The agent core lives in
//! `rivo-agent` and knows nothing about the UI; this crate is only the native window.

mod demo;
mod remote;
mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use rivo_agent::{
    AgentSettings, ChatInput, Event, EventKind, KILO_FREE_MODELS, Message, ModelConfig,
    PermissionMode, Preset, Protocol, ProviderConfig, Role, RunConfig, Session, Skill, TokenUsage,
    catalog::{self, CatalogModel, Category},
    checkpoint::Checkpoint,
    context, info::{McpServer, ModifiedFile},
    local::{self, LocalModel, ScanReport},
    registry::{self, RegistryItem},
};
use rune::{AppSpec, Chrome, Color, Cx, El, KeyEvent, Spring, TextEdit, Tx, col, row};
use rune_kit::shell::{PaneMotion, PaneState, SidebarPeek, SidebarToggle, SplitView};
use rune_kit::ControlSize;

pub const PROJECTS: [(&str, f32); 4] = [("rivo", 22.0), ("rune", 250.0), ("site", 150.0), ("notes", 320.0)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Provider,
    Connection,
    Local,
    Remote,
    McpMarketplace,
    SkillsMarketplace,
    Skills,
    Tools,
    Permissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorTab {
    Info,
    Checkpoints,
    Details,
    Tools,
    Skills,
}

/// Accumulated numbers for one model in the inspector's Models table.
#[derive(Clone, Debug, Default)]
pub struct ModelStat {
    pub name: String,
    pub steps: usize,
    pub usage: TokenUsage,
}

/// Workspace facts gathered on a worker thread.
#[derive(Clone, Debug, Default)]
struct InfoSnapshot {
    files: Vec<ModifiedFile>,
    mcp: Vec<McpServer>,
    checkpoints: Vec<Checkpoint>,
}

/// Result of a marketplace search or a one-click install, sent from a worker thread.
enum RegistryResult {
    Found { items: Vec<RegistryItem>, note: String },
    Installed { message: String, skills: Vec<Skill> },
    Failed(String),
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
    pub model_picker_open: bool,
    pub model_search: TextEdit,
    pub connection_status: String,
    pub testing: bool,
    // Marketplaces (MCP registry + skills.sh)
    pub marketplace_search: TextEdit,
    pub registry_items: Vec<RegistryItem>,
    pub registry_status: String,
    pub registry_busy: bool,
    pub installed_ids: Vec<String>,
    registry_rx: Option<mpsc::Receiver<RegistryResult>>,
    // models.dev catalog (all model + context config lives in the Providers tab)
    pub catalog_models: Vec<CatalogModel>,
    pub catalog_query: TextEdit,
    pub catalog_category: Category,
    pub catalog_status: String,
    pub catalog_busy: bool,
    catalog_rx: Option<mpsc::Receiver<Result<Vec<CatalogModel>, String>>>,
    // Official models.dev provider logos, downloaded once and cached for 24h by the agent.
    image_cache: Option<rune::ImageCache>,
    pub provider_images: HashMap<String, rune::Image>,
    /// Headless scenes run before `AppSpec::setup`; poll loads cached logos once it is available.
    seed_logos: bool,
    logos_rx: Option<mpsc::Receiver<Vec<(String, Vec<u8>)>>>,
    pub logos_status: String,
    // Local model discovery (localhost + same-Wi-Fi servers)
    pub local_models: Vec<LocalModel>,
    pub local_status: String,
    pub local_busy: bool,
    pub local_scan_lan: bool,
    pub local_host: TextEdit,
    pub local_scanned: bool,
    local_rx: Option<mpsc::Receiver<ScanReport>>,
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
    // Info centre
    pub token_usage: TokenUsage,
    pub model_stats: Vec<ModelStat>,
    pub modified_files: Vec<ModifiedFile>,
    pub mcp_servers: Vec<McpServer>,
    pub info_sections: HashMap<String, bool>,
    info_rx: Option<mpsc::Receiver<InfoSnapshot>>,
    last_info_refresh: f64,
    // Checkpoints
    pub checkpoints: Vec<Checkpoint>,
    pub checkpoint_label: TextEdit,
    pub checkpoint_status: String,
    pub checkpoint_busy: bool,
    pub checkpoint_undo: Option<String>,
    checkpoint_action_rx: Option<mpsc::Receiver<Result<String, String>>>,
    // Remote device bridge (phone app over LAN)
    pub remote_enabled: bool,
    pub remote_port: TextEdit,
    pub remote_token: String,
    pub remote_devices: Vec<remote::RemoteDevice>,
    pub remote_status: String,
    remote_tx: Option<mpsc::Sender<remote::RemoteCommand>>,
    remote_rx: Option<mpsc::Receiver<remote::RemoteCommand>>,
    remote_server: Option<remote::RemoteServer>,
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
        // Already-installed items show as "Added" in both marketplaces after a restart.
        let mut installed_ids: Vec<String> = rivo_agent::info::mcp_servers(&cwd)
            .into_iter().map(|s| s.name).collect();
        installed_ids.extend(skills.iter().map(|s| s.name.clone()));
        let (remote_enabled, remote_port, remote_token) = load_remote_config();
        let (remote_tx, remote_rx) = mpsc::channel();
        let mut remote_server = None;
        let mut remote_status = String::new();
        if remote_enabled {
            match remote::RemoteServer::start(remote_port, remote_token.clone(), remote_tx.clone()) {
                Ok(server) => {
                    remote_server = Some(server);
                    remote_status = format!("Bridge listening on {}", remote::lan_address(remote_port));
                }
                Err(e) => remote_status = format!("Bridge failed: {e}"),
            }
        }
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
            model_picker_open: false,
            model_search: TextEdit::new(""),
            connection_status: String::new(),
            testing: false,
            marketplace_search: TextEdit::new(""),
            registry_items: Vec::new(),
            registry_status: String::new(),
            registry_busy: false,
            installed_ids,
            registry_rx: None,
            catalog_models: Vec::new(),
            catalog_query: TextEdit::new(""),
            catalog_category: Category::All,
            catalog_status: String::new(),
            catalog_busy: false,
            catalog_rx: None,
            image_cache: None,
            provider_images: HashMap::new(),
            seed_logos: false,
            logos_rx: None,
            logos_status: String::new(),
            local_models: Vec::new(),
            local_status: String::new(),
            local_busy: false,
            local_scan_lan: false,
            local_host: TextEdit::new(""),
            local_scanned: false,
            local_rx: None,
            model_config,
            agent_settings,
            skills,
            inspector_tab: InspectorTab::Info,
            context_window_edit: TextEdit::new(window.to_string()),
            max_output_edit: TextEdit::new(max_output.to_string()),
            temperature_edit: TextEdit::new(format!("{temperature}")),
            system_prompt: TextEdit::new(system_prompt_text),
            context_used: 0,
            context_window: window,
            compaction_notice: None,
            token_usage: TokenUsage::default(),
            model_stats: Vec::new(),
            modified_files: Vec::new(),
            mcp_servers: Vec::new(),
            info_sections: HashMap::new(),
            info_rx: None,
            last_info_refresh: -1.0,
            checkpoints: Vec::new(),
            checkpoint_label: TextEdit::new(""),
            checkpoint_status: String::new(),
            checkpoint_busy: false,
            checkpoint_undo: None,
            checkpoint_action_rx: None,
            remote_enabled: remote_server.is_some(),
            remote_port: TextEdit::new(remote_port.to_string()),
            remote_token,
            remote_devices: Vec::new(),
            remote_status,
            remote_tx: Some(remote_tx),
            remote_rx: Some(remote_rx),
            remote_server,
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

    pub fn set_inspector_tab(&mut self, tab: InspectorTab) {
        self.inspector_tab = tab;
        if tab == InspectorTab::Info || tab == InspectorTab::Checkpoints {
            self.refresh_info();
        }
    }

    /// Sections start expanded; the map holds only the ones the user collapsed.
    pub fn info_open(&self, key: &str) -> bool {
        !self.info_sections.get(key).is_some_and(|open| !open)
    }

    pub fn toggle_info_section(&mut self, key: &str) {
        let open = self.info_open(key);
        self.info_sections.insert(key.to_string(), !open);
    }

    /// Git and MCP discovery touch the filesystem, so they run on a worker thread.
    fn refresh_info(&mut self) {
        if self.info_rx.is_some() {
            return;
        }
        let cwd = self.current().cwd.clone();
        let (tx, rx) = mpsc::channel();
        self.info_rx = Some(rx);
        thread::spawn(move || {
            let snapshot = InfoSnapshot {
                files: rivo_agent::info::modified_files(&cwd),
                mcp: rivo_agent::info::mcp_servers(&cwd),
                checkpoints: rivo_agent::checkpoint::list(&cwd).unwrap_or_default(),
            };
            let _ = tx.send(snapshot);
        });
    }

    pub fn create_checkpoint(&mut self) {
        if self.checkpoint_action_rx.is_some() || self.checkpoint_busy {
            return;
        }
        let label = self.checkpoint_label.text.trim().to_string();
        let cwd = self.current().cwd.clone();
        let (tx, rx) = mpsc::channel();
        self.checkpoint_action_rx = Some(rx);
        self.checkpoint_busy = true;
        self.checkpoint_status = "Saving checkpoint…".into();
        thread::spawn(move || {
            let result = rivo_agent::checkpoint::create(&cwd, &label)
                .map(|c| format!("Checkpoint {} saved: {}", c.id, c.label));
            let _ = tx.send(result);
        });
    }

    pub fn restore_checkpoint(&mut self, id: &str) {
        if self.checkpoint_action_rx.is_some() || self.checkpoint_busy || id.is_empty() {
            return;
        }
        let id = id.to_string();
        let cwd = self.current().cwd.clone();
        let (tx, rx) = mpsc::channel();
        self.checkpoint_action_rx = Some(rx);
        self.checkpoint_busy = true;
        self.checkpoint_status = format!("Restoring {id}…");
        thread::spawn(move || {
            let _ = tx.send(rivo_agent::checkpoint::restore(&cwd, &id));
        });
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

    /// The inspector's three text fields drive both custom and discovered local endpoints.
    fn sync_custom_fields(&mut self) {
        if matches!(self.config.preset, Preset::Custom | Preset::Local) {
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
        self.close_model_picker();
        self.context_window_edit = TextEdit::new(self.model_config.context_window.to_string());
        self.max_output_edit = TextEdit::new(self.model_config.max_output.to_string());
        self.temperature_edit = TextEdit::new(format!("{}", self.model_config.temperature));
        self.system_prompt = TextEdit::new(self.agent_settings.system_prompt.clone());
        self.connection_status.clear();
        self.settings_open = true;
        if self.settings_tab == SettingsTab::Local && !self.local_scanned {
            self.scan_local();
        }
    }

    pub fn choose_tab(&mut self, tab: SettingsTab) {
        let marketplace = matches!(tab, SettingsTab::McpMarketplace | SettingsTab::SkillsMarketplace);
        if marketplace && self.settings_tab != tab {
            self.registry_items.clear();
            self.registry_status.clear();
        }
        self.settings_tab = tab;
        self.settings_select_open = None;
        self.connection_status.clear();
        if marketplace && self.registry_items.is_empty() && !self.registry_busy {
            self.search_registry();
        }
        if tab == SettingsTab::Connection && self.catalog_models.is_empty() && !self.catalog_busy {
            self.load_catalog();
        }
        if tab == SettingsTab::Local && !self.local_scanned {
            self.scan_local();
        }
    }

    /// Show the cached catalog immediately, then refresh it on a worker thread. A slow or
    /// unreachable models.dev must never leave the Providers screen empty.
    pub fn load_catalog(&mut self) {
        if self.catalog_rx.is_some() {
            return;
        }
        let cached = catalog::cached();
        let had_cache = cached.is_some();
        if let Some(models) = cached {
            let ids: Vec<String> = models.iter()
                .flat_map(|m| [m.lab.clone(), m.provider.clone()])
                .collect::<std::collections::HashSet<_>>().into_iter().collect();
            self.load_logos(ids);
            self.catalog_models = models;
            self.catalog_status = if catalog::is_fresh() {
                "models.dev cache is up to date".into()
            } else {
                "models.dev cache — refreshing in the background…".into()
            };
        } else {
            self.catalog_status = "Loading models.dev catalog…".into();
        }
        let (tx, rx) = mpsc::channel();
        self.catalog_rx = Some(rx);
        self.catalog_busy = true;
        thread::spawn(move || {
            let result = if had_cache { catalog::refresh() } else { catalog::load() };
            let _ = tx.send(result);
        });
    }

    /// Download the official models.dev logos for the catalog's providers. Reads the 24-hour
    /// disk cache first, so this only touches the network on a cold start or after expiry.
    fn load_logos(&mut self, ids: Vec<String>) {
        if self.logos_rx.is_some() || ids.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.logos_rx = Some(rx);
        self.logos_status = "Loading provider logos…".into();
        thread::spawn(move || {
            let _ = tx.send(catalog::ensure_logos(&ids));
        });
    }

    /// Probe standard local-model ports on a worker thread. With `local_scan_lan` enabled the
    /// scan also walks this machine's /24, so servers on the same Wi-Fi are found.
    pub fn scan_local(&mut self) {
        if self.local_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.local_rx = Some(rx);
        self.local_busy = true;
        self.local_scanned = true;
        self.local_status = "Scanning local model ports…".into();
        let include_lan = self.local_scan_lan;
        let host = self.local_host.text.trim().to_string();
        thread::spawn(move || {
            let _ = tx.send(local::scan(include_lan, &host));
        });
    }

    pub fn toggle_local_lan(&mut self) {
        self.local_scan_lan = !self.local_scan_lan;
        // Rescan right away so switching the toggle is the only click needed.
        self.scan_local();
    }

    pub fn set_catalog_category(&mut self, category: Category) {
        self.catalog_category = category;
    }

    /// Shared one-click configuration for catalog and locally discovered models.
    fn apply_provider(&mut self, config: ProviderConfig, protocol: Protocol, context_window: usize, max_output: usize, status: String) {
        self.config = config;
        self.settings_preset = self.config.preset;
        self.model_config.protocol = protocol;
        self.model_config.base_hint = self.config.base_url.clone();
        self.model_config.context_window = context_window.max(4_096);
        self.model_config.max_output = max_output.clamp(256, 131_072);
        let used = context::conversation_tokens(&self.current().messages);
        self.base_url = TextEdit::new(self.config.base_url.clone());
        self.api_key = TextEdit::new(self.config.api_key.clone());
        self.model_name = TextEdit::new(self.config.model.clone());
        self.context_window_edit = TextEdit::new(self.model_config.context_window.to_string());
        self.max_output_edit = TextEdit::new(self.model_config.max_output.to_string());
        self.context_window = self.model_config.context_window;
        self.context_used = used;
        self.connection_status = status;
        save_config(&self.config, &self.model_config, &self.agent_settings);
    }

    /// Apply a models.dev entry: protocol, endpoint, model id, context and output limits.
    /// The API key is taken from the environment variable named by the provider when present.
    pub fn use_catalog_model(&mut self, index: usize) {
        let query = self.catalog_query.text.clone();
        let category = self.catalog_category;
        let Some(model) = catalog::filter(&self.catalog_models, &query, category).get(index).map(|m| (*m).clone()) else { return };
        let key = if model.env.is_empty() { String::new() } else {
            std::env::var(&model.env).unwrap_or_default()
        };
        let status = format!("Using {} · {}", model.provider_name, model.name);
        self.apply_provider(
            ProviderConfig::custom(model.base_url.clone(), key, model.id.clone()),
            model.protocol,
            model.context,
            model.max_output,
            status,
        );
    }

    /// One-click use of a discovered local model: endpoint, model id and a safe context window.
    pub fn use_local_model(&mut self, index: usize) {
        let Some(model) = self.local_models.get(index).cloned() else { return };
        let status = format!("Using {} · {} at {}", model.backend, model.name, model.address());
        let max_output = self.model_config.max_output;
        self.apply_provider(
            ProviderConfig::local(model.base_url.clone(), model.id.clone()),
            Protocol::OpenAiCompatible,
            model.context,
            max_output,
            status,
        );
    }

    /// Fetch the active marketplace in the background; never block the UI thread.
    pub fn search_registry(&mut self) {
        if self.registry_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.registry_rx = Some(rx);
        self.registry_busy = true;
        self.registry_status = "Searching…".into();
        let mcp = self.settings_tab == SettingsTab::McpMarketplace;
        let query = self.marketplace_search.text.trim().to_string();
        thread::spawn(move || {
            let result = if mcp {
                registry::search_mcp(&query, 40).map(|items| {
                    let note = if query.is_empty() {
                        "Official MCP Registry · namespace-verified publishers · metadata only".to_string()
                    } else {
                        format!("Official MCP Registry · results for “{query}”")
                    };
                    RegistryResult::Found { items, note }
                })
            } else {
                registry::search_skills(&query, 30).map(|items| {
                    let note = "skills.sh · public GitHub skills with audits and install counts".to_string();
                    RegistryResult::Found { items, note }
                })
            };
            let _ = tx.send(result.unwrap_or_else(RegistryResult::Failed));
        });
    }

    /// One-click install: writes an MCP config entry or downloads a skill into the workspace.
    /// Runs on a worker thread and returns the refreshed skill list for the UI.
    pub fn install_registry_item(&mut self, index: usize) {
        if self.registry_busy {
            return;
        }
        let Some(item) = self.registry_items.get(index).cloned() else { return };
        if self.installed_ids.contains(&item.id) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.registry_rx = Some(rx);
        self.registry_busy = true;
        self.registry_status = format!("Installing {}…", item.name);
        let cwd = self.current().cwd.clone();
        thread::spawn(move || {
            let result = if matches!(item.install, registry::InstallSpec::Skill { .. }) {
                registry::install_skill(&cwd, &item).map(|message| RegistryResult::Installed {
                    message,
                    skills: rivo_agent::skills::discover(&cwd),
                })
            } else {
                registry::install_mcp(&item).map(|message| RegistryResult::Installed {
                    message,
                    skills: rivo_agent::skills::discover(&cwd),
                })
            };
            let _ = tx.send(result.unwrap_or_else(RegistryResult::Failed));
        });
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

    pub fn toggle_model_picker(&mut self) {
        if self.model_picker_open {
            self.close_model_picker();
        } else {
            self.model_search = TextEdit::new("");
            self.model_search.focused = true;
            self.model_picker_open = true;
        }
    }

    pub fn close_model_picker(&mut self) {
        self.model_picker_open = false;
        self.model_search.focused = false;
        self.model_search = TextEdit::new("");
    }

    /// Switch the active model from the composer picker. Picking a Kilo free model also
    /// switches the provider to the free gateway, so the composer is enough for a quick change.
    pub fn choose_model(&mut self, id: &str) {
        // Discovered local models use `base_url|model` as their picker id, so two servers can
        // serve the same model name without colliding.
        if let Some(model) = self.local_models.iter().find(|m| m.key() == id).cloned() {
            let status = format!("Using {} · {} at {}", model.backend, model.name, model.address());
            let max_output = self.model_config.max_output;
            self.apply_provider(
                ProviderConfig::local(model.base_url.clone(), model.id.clone()),
                Protocol::OpenAiCompatible,
                model.context,
                max_output,
                status,
            );
            self.close_model_picker();
            return;
        }
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
            self.close_model_picker();
            save_config(&self.config, &self.model_config, &self.agent_settings);
        }
    }

    /// The config the settings form currently describes, before pressing Apply.
    fn pending_config(&self) -> ProviderConfig {
        match self.settings_preset {
            Preset::Mock => ProviderConfig::mock(),
            Preset::KiloFree => ProviderConfig::kilo_free(KILO_FREE_MODELS[self.free_index].id),
            Preset::Local => ProviderConfig::local(
                self.base_url.text.trim().to_owned(),
                self.model_name.text.trim().to_owned(),
            ),
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

    // ---------------------------------------------------------------- remote bridge

    pub fn toggle_remote(&mut self) {
        if let Some(mut server) = self.remote_server.take() {
            server.stop();
            self.remote_enabled = false;
            self.remote_status = "Bridge stopped.".into();
            self.persist_remote();
            return;
        }
        let port: u16 = self.remote_port.text.trim().parse().unwrap_or(8787).min(65535);
        let tx = match self.remote_tx.clone() {
            Some(tx) => tx,
            None => {
                let (tx, rx) = mpsc::channel();
                self.remote_tx = Some(tx.clone());
                self.remote_rx = Some(rx);
                tx
            }
        };
        match remote::RemoteServer::start(port, self.remote_token.clone(), tx) {
            Ok(server) => {
                self.remote_server = Some(server);
                self.remote_enabled = true;
                self.remote_status = format!("Bridge listening on {}", remote::lan_address(port));
            }
            Err(e) => self.remote_status = format!("Bridge failed: {e}"),
        }
        self.persist_remote();
    }

    pub fn regenerate_remote_code(&mut self) {
        self.remote_token = remote::new_code();
        self.remote_devices.clear();
        self.remote_status = "New pairing code generated; previous devices are unpaired.".into();
        if self.remote_enabled {
            self.toggle_remote();
            self.toggle_remote();
        }
        self.persist_remote();
    }

    fn persist_remote(&self) {
        let path = config_path();
        let mut v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_default()
        ).unwrap_or_else(|_| serde_json::json!({}));
        v["remote_enabled"] = serde_json::json!(self.remote_enabled);
        v["remote_port"] = serde_json::json!(self.remote_port.text.trim().parse::<u16>().unwrap_or(8787));
        v["remote_token"] = serde_json::json!(self.remote_token);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default()) {
            eprintln!("rivo: could not save remote settings: {e}");
        }
    }

    fn handle_remote(&mut self, cmd: remote::RemoteCommand) {
        use remote::RemoteCommand as C;
        match cmd {
            C::Status { reply } => {
                let value = serde_json::json!({
                    "name": "Rivo",
                    "sessions": self.sessions.len(),
                    "devices": self.remote_devices.len(),
                    "tools": rivo_agent::tools::all_tools().len(),
                    "model": self.config.model,
                });
                let _ = reply.send(Ok(value));
            }
            C::Pair { name, ip, reply } => {
                let since = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let device = remote::RemoteDevice { name: name.clone(), ip: ip.clone(), since: since.to_string() };
                if !self.remote_devices.iter().any(|d| d.ip == ip && d.name == name) {
                    self.remote_devices.push(device);
                }
                self.remote_status = format!("{name} ({ip}) paired.");
                let _ = reply.send(Ok(serde_json::json!({ "token": self.remote_token, "name": name })));
            }
            C::ListSessions { reply } => {
                let value = serde_json::json!(self.sessions.iter().map(|s| serde_json::json!({
                    "id": s.id, "title": s.title, "project": s.project, "busy": s.busy,
                })).collect::<Vec<_>>());
                let _ = reply.send(Ok(value));
            }
            C::GetSession { id, reply } => {
                let value = self.sessions.iter().find(|s| s.id == id).map(session_json);
                let _ = reply.send(value.ok_or_else(|| "session not found".into()));
            }
            C::NewSession { title, reply } => {
                let id = format!("s{}", self.next_id);
                self.next_id += 1;
                let cwd = self.current().cwd.clone();
                let project = cwd.file_name().and_then(|s| s.to_str()).unwrap_or("workspace").to_string();
                let session = Session::new(id.clone(), title, project, cwd);
                let value = session_json(&session);
                self.sessions.push(session);
                let _ = reply.send(Ok(value));
            }
            C::Send { id, text, reply } => {
                let ok = self.start_turn(&id, &text);
                let _ = reply.send(if ok { Ok(serde_json::json!({ "started": true, "id": id })) }
                    else { Err("session is busy or missing".into()) });
            }
            C::Checkpoints { reply } => {
                let cwd = self.current().cwd.clone();
                let (rtx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let result = rivo_agent::checkpoint::list(&cwd).map(|items| serde_json::json!(
                        items.iter().map(|c| serde_json::json!({ "id": c.id, "label": c.label, "when": c.when })).collect::<Vec<_>>()
                    ));
                    let _ = rtx.send(result);
                });
                self.reply_later(reply, rx);
            }
            C::SaveCheckpoint { label, reply } => {
                let cwd = self.current().cwd.clone();
                let (rtx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let result = rivo_agent::checkpoint::create(&cwd, &label).map(|c| serde_json::json!({
                        "id": c.id, "label": c.label, "when": c.when,
                    }));
                    let _ = rtx.send(result);
                });
                self.reply_later(reply, rx);
                self.refresh_info();
            }
            C::RestoreCheckpoint { id, reply } => {
                let cwd = self.current().cwd.clone();
                let (rtx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let result = rivo_agent::checkpoint::restore(&cwd, &id)
                        .map(|message| serde_json::json!({ "message": message }));
                    let _ = rtx.send(result);
                });
                self.reply_later(reply, rx);
                self.refresh_info();
            }
        }
    }

    fn reply_later(&self, reply: mpsc::Sender<Result<serde_json::Value, String>>,
                   rx: mpsc::Receiver<Result<serde_json::Value, String>>) {
        thread::spawn(move || {
            let result = rx.recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|e| Err(format!("checkpoint failed: {e}")));
            let _ = reply.send(result);
        });
    }

    pub fn submit(&mut self) {
        let prompt = self.draft.text.trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let id = self.selected.clone();
        if self.start_turn(&id, &prompt) {
            self.sync_custom_fields();
            self.draft = TextEdit::new("");
        }
    }

    /// Start a turn on any session — used by the composer and by the phone bridge.
    pub fn start_turn(&mut self, id: &str, prompt: &str) -> bool {
        let prompt = prompt.trim().to_owned();
        if prompt.is_empty() {
            return false;
        }
        let Some(idx) = self.sessions.iter().position(|s| s.id == id) else { return false };
        if self.sessions[idx].busy {
            return false;
        }
        self.turn += 1;
        let turn = self.turn;
        self.turns.insert(id.to_string(), turn);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.insert(id.to_string(), cancel.clone());

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
        let _ = rivo_agent::engine::spawn(id.to_string(), turn, cancel, input, run, self.engine_tx.clone());
        true
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
            EventKind::ModelStep { model } => {
                if let Some(stat) = self.model_stats.iter_mut().find(|m| m.name == model) {
                    stat.steps += 1;
                } else {
                    self.model_stats.push(ModelStat { name: model, steps: 1, usage: TokenUsage::default() });
                }
            }
            EventKind::ModelUsage { model, usage } => {
                self.token_usage.add(usage);
                if let Some(stat) = self.model_stats.iter_mut().find(|m| m.name == model) {
                    stat.usage.add(usage);
                } else {
                    self.model_stats.push(ModelStat { name: model, steps: 0, usage });
                }
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

/// Remote bridge settings live in the same config file, but are loaded separately so the
/// existing tuple type stays unchanged.
fn load_remote_config() -> (bool, u16, String) {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(config_path()).unwrap_or_default()
    ).unwrap_or_default();
    let port = v.get("remote_port").and_then(serde_json::Value::as_u64)
        .unwrap_or(8787).min(65535) as u16;
    let token = v.get("remote_token").and_then(serde_json::Value::as_str)
        .filter(|s| s.len() >= 8).map(str::to_string).unwrap_or_else(remote::new_code);
    let enabled = v.get("remote_enabled").and_then(serde_json::Value::as_bool).unwrap_or(false);
    (enabled, port, token)
}

fn load_config() -> Option<SavedConfig> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let get = |k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let preset = match v.get("preset").and_then(serde_json::Value::as_str).unwrap_or("") {
        "mock" => Preset::Mock,
        "kilo" => Preset::KiloFree,
        "local" => Preset::Local,
        "custom" => Preset::Custom,
        _ => return None,
    };
    let provider = match preset {
        Preset::Mock => ProviderConfig::mock(),
        Preset::KiloFree => ProviderConfig::kilo_free(&get("model")),
        Preset::Local => ProviderConfig::local(get("base_url"), get("model")),
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
        Preset::Local => "local",
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
    let mut json = serde_json::json!({
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
    // Keep the remote bridge settings when saving model/context changes.
    if let Ok(old) = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(config_path()).unwrap_or_default()) {
        for key in ["remote_enabled", "remote_port", "remote_token"] {
            if let Some(value) = old.get(key) {
                json[key] = value.clone();
            }
        }
    }
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

/// JSON form of a session for the phone bridge.
fn session_json(s: &Session) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "title": s.title,
        "project": s.project,
        "busy": s.busy,
        "messages": s.messages.iter().map(|m| serde_json::json!({
            "role": format!("{:?}", m.role).to_lowercase(),
            "text": m.text,
            "tools": m.tools.iter().map(|t| serde_json::json!({
                "name": t.name, "ok": t.ok, "finished": t.finished, "result": t.result,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
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

fn poll(s: &mut Rivo, clock: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if s.seed_logos {
        s.seed_logos = false;
        if let Some(cache) = &s.image_cache {
            let ids: Vec<String> = s.catalog_models.iter()
                .flat_map(|m| [m.lab.clone(), m.provider.clone()])
                .collect::<std::collections::HashSet<_>>().into_iter().collect();
            let images: Vec<(String, rune::Image)> = ids.into_iter()
                .filter_map(|id| {
                    let bytes = catalog::cached_logo_bytes(&id)?;
                    Some((id, cache.svg(bytes, 22.0, 2.0)))
                })
                .collect();
            s.logos_status = format!("{} provider logos cached for 24h", images.len());
            s.provider_images = images.into_iter().collect();
            changed = true;
        }
    }
    while let Ok(ev) = s.events.try_recv() {
        changed |= s.apply_event(ev);
    }
    if let Some(rx) = s.remote_rx.take() {
        while let Ok(cmd) = rx.try_recv() {
            s.handle_remote(cmd);
            changed = true;
        }
        s.remote_rx = Some(rx);
    }
    if let Some(rx) = &s.info_rx {
        if let Ok(snapshot) = rx.try_recv() {
            // A periodic refresh usually returns the same facts; only a real difference
            // should ask Rune to redraw (see the idle-poll perf warning).
            let differs = s.modified_files != snapshot.files
                || s.mcp_servers != snapshot.mcp
                || s.checkpoints != snapshot.checkpoints;
            s.modified_files = snapshot.files;
            s.mcp_servers = snapshot.mcp;
            s.checkpoints = snapshot.checkpoints;
            s.info_rx = None;
            changed |= differs;
        }
    }
    let info_visible = s.inspector.is_open()
        && (s.inspector_tab == InspectorTab::Info || s.inspector_tab == InspectorTab::Checkpoints);
    if info_visible && clock - s.last_info_refresh > 2.0 {
        s.last_info_refresh = clock;
        s.refresh_info();
    }
    if let Some(rx) = &s.checkpoint_action_rx {
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(message) => {
                    if message.starts_with("Checkpoint ") {
                        s.checkpoint_label = TextEdit::new("");
                    }
                    // The message contains the safety tag whenever a restore happened.
                    s.checkpoint_undo = message.split("rivo-safety-").nth(1)
                        .map(|rest| format!("rivo-safety-{}", rest.split_whitespace().next().unwrap_or("")));
                    s.checkpoint_status = message;
                    s.refresh_info();
                }
                Err(message) => s.checkpoint_status = message,
            }
            s.checkpoint_busy = false;
            s.checkpoint_action_rx = None;
            changed = true;
        }
    }
    if let Some(rx) = &s.catalog_rx {
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(models) => {
                    let count = models.len();
                    let providers = models.iter().map(|m| m.provider.as_str())
                        .collect::<std::collections::HashSet<_>>().len();
                    let ids: Vec<String> = models.iter()
                        .flat_map(|m| [m.lab.clone(), m.provider.clone()])
                        .collect::<std::collections::HashSet<_>>().into_iter().collect();
                    s.catalog_status = format!("{count} models from {providers} providers · models.dev · cached for 24h");
                    s.load_logos(ids);
                    s.catalog_models = models;
                }
                Err(message) => {
                    let count = s.catalog_models.len();
                    s.catalog_status = if count > 0 {
                        format!("{count} models from cache — models.dev refresh failed: {message}")
                    } else {
                        format!("Could not load models.dev: {message}")
                    };
                }
            }
            s.catalog_busy = false;
            s.catalog_rx = None;
            changed = true;
        }
    }
    if let Some(rx) = &s.logos_rx {
        if let Ok(entries) = rx.try_recv() {
            let images: Vec<(String, rune::Image)> = match &s.image_cache {
                Some(cache) => entries.into_iter()
                    .map(|(id, bytes)| (id, cache.svg(bytes, 22.0, 2.0)))
                    .collect(),
                None => Vec::new(),
            };
            s.logos_status = format!("{} provider logos cached for 24h", images.len());
            s.provider_images = images.into_iter().collect();
            s.logos_rx = None;
            changed = true;
        }
    }
    if let Some(rx) = &s.local_rx {
        if let Ok(report) = rx.try_recv() {
            s.local_status = report.summary();
            s.local_models = report.models;
            s.local_busy = false;
            s.local_rx = None;
            changed = true;
        }
    }
    if let Some(rx) = &s.registry_rx {
        if let Ok(result) = rx.try_recv() {
            match result {
                RegistryResult::Found { items, note } => {
                    s.registry_items = items;
                    s.registry_status = note;
                }
                RegistryResult::Installed { message, skills } => {
                    s.skills = skills;
                    if let Some(id) = s.registry_items.iter().find(|i| message.contains(&i.name)).map(|i| i.id.clone()) {
                        if !s.installed_ids.contains(&id) { s.installed_ids.push(id); }
                    }
                    // An added MCP server should show up in the right-hand panel at once.
                    s.refresh_info();
                    s.registry_status = message;
                }
                RegistryResult::Failed(message) => {
                    s.registry_status = format!("Error: {message}");
                }
            }
            s.registry_busy = false;
            s.registry_rx = None;
            changed = true;
        }
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
    if s.busy() || s.testing || s.registry_busy || s.catalog_busy || s.logos_rx.is_some() || s.local_busy || s.checkpoint_busy {
        tx.wake_after(ms(16));
    }
    if s.remote_enabled {
        // The bridge runs on another thread; poll must run so its requests get answered.
        tx.wake_after(ms(60));
    }
    if info_visible {
        // Keep counts and the working-tree view warm while the info centre is open.
        tx.wake_after(ms(500));
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
    // Headless scenes draw one frame, so seed the async info-centre data here.
    let cwd = PathBuf::from(".");
    s.modified_files = rivo_agent::info::modified_files(&cwd);
    s.mcp_servers = rivo_agent::info::mcp_servers(&cwd);
    s.checkpoints = rivo_agent::checkpoint::list(&cwd).unwrap_or_default();
}

fn scene_checkpoints(s: &mut Rivo) {
    scene_demo(s);
    s.inspector_tab = InspectorTab::Checkpoints;
    s.checkpoints = vec![
        Checkpoint { id: "a1b2c3d".into(), label: "before the UI refactor".into(), when: "2026-11-20T10:14:02Z".into() },
        Checkpoint { id: "e4f5g6h".into(), label: "after adding marketplaces".into(), when: "2026-11-20T16:42:31Z".into() },
        Checkpoint { id: "i7j8k9l".into(), label: "models.dev catalog working".into(), when: "2026-11-21T09:05:17Z".into() },
    ];
    s.checkpoint_status = "3 checkpoints · newest first".into();
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

fn scene_remote(s: &mut Rivo) {
    scene_settings(s);
    s.settings_tab = SettingsTab::Remote;
    let port: u16 = 8799;
    let token = "test-pairing-code-1234".to_string();
    let (tx, rx) = mpsc::channel();
    s.remote_tx = Some(tx.clone());
    s.remote_rx = Some(rx);
    s.remote_enabled = true;
    s.remote_port = TextEdit::new(port.to_string());
    s.remote_token = token.clone();
    match remote::RemoteServer::start(port, token, tx) {
        Ok(server) => s.remote_server = Some(server),
        Err(_) => s.remote_enabled = false,
    }
    s.remote_status = format!("Bridge listening on {}", remote::lan_address(port));
    s.remote_devices = vec![remote::RemoteDevice {
        name: "Pixel".into(), ip: "192.168.1.24".into(), since: "1763712000".into(),
    }];
}

fn scene_providers(s: &mut Rivo) {
    scene_settings(s);
    s.settings_tab = SettingsTab::Connection;
    s.catalog_category = Category::Free;
    // Use the real cached catalog so the scene also exercises the quick cache path.
    s.load_catalog();
    // Scenes run before setup; poll picks up the cached logos once the image cache exists.
    s.seed_logos = true;
}

fn scene_local(s: &mut Rivo) {
    scene_settings(s);
    s.settings_tab = SettingsTab::Local;
    s.local_scanned = true;
    s.local_models = vec![
        LocalModel {
            id: "qwen2.5-coder:7b".into(), name: "Qwen2.5 Coder 7B".into(), backend: "Ollama".into(),
            host: "127.0.0.1".into(), port: 11434, base_url: "http://127.0.0.1:11434/v1".into(),
            context: 32_768, local: true,
        },
        LocalModel {
            id: "local-model".into(), name: "Local model".into(), backend: "LM Studio".into(),
            host: "127.0.0.1".into(), port: 1234, base_url: "http://127.0.0.1:1234/v1".into(),
            context: 16_384, local: true,
        },
        LocalModel {
            id: "meta-llama-3.1-8b".into(), name: "Llama 3.1 8B".into(), backend: "llama.cpp".into(),
            host: "192.168.1.42".into(), port: 8080, base_url: "http://192.168.1.42:8080/v1".into(),
            context: 8_192, local: false,
        },
    ];
    s.local_status = format!("{} models from 3 servers · localhost + 192.168.1.0/24", s.local_models.len());
}

fn scene_marketplace(s: &mut Rivo) {
    scene_settings(s);
    s.settings_tab = SettingsTab::McpMarketplace;
    s.registry_items = vec![
        RegistryItem {
            id: "io.github.example/filesystem".into(),
            name: "Filesystem".into(),
            description: "Read and write files inside a configured root directory.".into(),
            publisher: "io.github.example".into(),
            installs: 0,
            verified: true,
            install: registry::InstallSpec::Command {
                program: "npx".into(),
                args: vec!["-y".into(), "@example/mcp-filesystem".into()],
                env: vec!["WORKSPACE_ROOT".into()],
            },
        },
        RegistryItem {
            id: "com.example/remote-search".into(),
            name: "Remote Search".into(),
            description: "Hosted web search over streamable HTTP.".into(),
            publisher: "com.example".into(),
            installs: 0,
            verified: true,
            install: registry::InstallSpec::Remote { url: "https://api.example.com/mcp".into() },
        },
    ];
    s.registry_status = "Official MCP Registry · namespace-verified publishers".into();
}

fn scene_skill_marketplace(s: &mut Rivo) {
    scene_settings(s);
    s.settings_tab = SettingsTab::SkillsMarketplace;
    s.registry_items = vec![RegistryItem {
        id: "mattpocock/skills/code-review".into(),
        name: "code-review".into(),
        description: "911335 installs · mattpocock/skills".into(),
        publisher: "mattpocock/skills".into(),
        installs: 911_335,
        verified: false,
        install: registry::InstallSpec::Skill {
            source: "mattpocock/skills".into(),
            skill_id: "code-review".into(),
        },
    }];
    s.registry_status = "skills.sh · public GitHub skills with audits and install counts".into();
}

fn main() {
    let spec = AppSpec {
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        scenes: &[
            ("demo", scene_demo),
            ("checkpoints", scene_checkpoints),
            ("streaming", scene_streaming),
            ("settings", scene_settings),
            ("remote", scene_remote),
            ("providers", scene_providers),
            ("local", scene_local),
            ("marketplace", scene_marketplace),
            ("skill-marketplace", scene_skill_marketplace),
        ],
        poll: Some(poll),
        on_key: Some(on_key),
        setup: Some(|s, waker| s.image_cache = Some(rune::ImageCache::new(waker))),
        ..AppSpec::new("Rivo — native coding agent", (1160.0, 760.0), Rivo::new, view)
    };
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 25.0)), drag_regions: true };
    rune::App::new(spec).chrome(chrome).run();
}
