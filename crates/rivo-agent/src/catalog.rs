//! models.dev catalog: a free, open-source database of 300+ providers and 10k models.
//!
//! Data comes from `https://models.dev/api.json` (no API key). The full file is ~7 MB, so it
//! is fetched on a worker thread and cached at `~/.config/rivo/models-dev.json` for 24 hours.
//! A stale cache is used if the network is unavailable.
//!
//! The official provider logos live beside that data in models.dev's public repository, one
//! `logo.svg` per lab/provider. They are downloaded once into `~/.config/rivo/logos/` and use
//! the same 24-hour cache as the catalog.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::model::Protocol;

const API: &str = "https://models.dev/api.json";
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
const LOGO_BASE: &str = "https://raw.githubusercontent.com/anomalyco/models.dev/dev";
const LOGO_WORKERS: usize = 8;

/// A model with everything Rivo needs to configure a provider call.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub description: String,
    /// API provider (sometimes a router, e.g. `agentrouter`, `302ai`).
    pub provider: String,
    pub provider_name: String,
    /// Lab that created the model (`anthropic`, `google`, …), from `canonical_model_id`.
    /// This is the id whose official logo belongs next to the model.
    pub lab: String,
    pub base_url: String,
    pub env: String,
    pub protocol: Protocol,
    pub context: usize,
    pub max_output: usize,
    pub input_cost: f64,
    pub output_cost: f64,
    pub free: bool,
    pub tool_call: bool,
    pub reasoning: bool,
    pub open_weights: bool,
}

/// Which subset of the catalog to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    All,
    Free,
    ToolCall,
    Reasoning,
    OpenWeights,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::All => "All",
            Category::Free => "Free",
            Category::ToolCall => "Tool-capable",
            Category::Reasoning => "Reasoning",
            Category::OpenWeights => "Open weights",
        }
    }

    pub fn all() -> [Category; 5] {
        [Category::All, Category::Free, Category::ToolCall, Category::Reasoning, Category::OpenWeights]
    }

    fn matches(self, m: &CatalogModel) -> bool {
        match self {
            Category::All => true,
            Category::Free => m.free,
            Category::ToolCall => m.tool_call,
            Category::Reasoning => m.reasoning,
            Category::OpenWeights => m.open_weights,
        }
    }
}

fn cache_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"))
        .join("rivo")
        .join("models-dev.json")
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn num_field(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn protocol_for(npm: &str) -> Protocol {
    let n = npm.to_lowercase();
    if n.contains("anthropic") {
        Protocol::Anthropic
    } else if n.contains("google") || n.contains("gemini") {
        Protocol::Gemini
    } else {
        Protocol::OpenAiCompatible
    }
}

fn base_url_for(npm: &str, provider: &str) -> String {
    match npm {
        "@ai-sdk/openai" => "https://api.openai.com/v1".into(),
        "@ai-sdk/anthropic" => "https://api.anthropic.com/v1".into(),
        "@ai-sdk/google" => "https://generativelanguage.googleapis.com/v1beta".into(),
        _ => provider.to_string(),
    }
}

fn parse(root: &Value) -> Vec<CatalogModel> {
    let mut out = Vec::new();
    for (id, provider) in root.as_object().into_iter().flatten() {
        let npm = str_field(provider, "npm");
        let api = str_field(provider, "api");
        let base_url = base_url_for(&npm, &api);
        let mut envs = provider.get("env").and_then(Value::as_array)
            .into_iter().flatten().filter_map(Value::as_str);
        let env = envs.next().unwrap_or("").to_string();
        let protocol = protocol_for(&npm);
        let provider_name = {
            let n = str_field(provider, "name");
            if n.is_empty() { id.clone() } else { n }
        };
        for (model_id, model) in provider.get("models").and_then(Value::as_object).into_iter().flatten() {
            let cost = &model["cost"];
            let input_cost = num_field(cost, "input");
            let output_cost = num_field(cost, "output");
            let limit = &model["limit"];
            let name = {
                let n = str_field(model, "name");
                if n.is_empty() { model_id.clone() } else { n }
            };
            // `canonical_model_id` is `lab/model-id` for models served through third-party
            // routers. Use the lab for the logo, not the router that happens to host it.
            let lab = str_field(model, "canonical_model_id")
                .split_once('/')
                .map(|(lab, _)| lab.to_string())
                .filter(|lab| !lab.is_empty())
                .unwrap_or_else(|| id.clone());
            out.push(CatalogModel {
                id: model_id.clone(),
                name,
                description: str_field(model, "description"),
                provider: id.clone(),
                provider_name: provider_name.clone(),
                lab,
                base_url: base_url.clone(),
                env: env.clone(),
                protocol,
                context: num_field(limit, "context") as usize,
                max_output: num_field(limit, "output") as usize,
                input_cost,
                output_cost,
                free: input_cost == 0.0 && output_cost == 0.0,
                tool_call: model.get("tool_call").and_then(Value::as_bool).unwrap_or(false),
                reasoning: model.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                open_weights: model.get("open_weights").and_then(Value::as_bool).unwrap_or(false),
            });
        }
    }
    out.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.name.cmp(&b.name)));
    out
}

fn read_cached() -> Option<Vec<CatalogModel>> {
    let path = cache_path();
    let text = fs::read_to_string(&path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(parse(&value))
}

fn cache_fresh() -> bool {
    let Ok(meta) = fs::metadata(cache_path()) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    SystemTime::now().duration_since(modified).map_or(false, |age| age < TTL)
}

/// The on-disk catalog, fresh or stale. Reading 6.5 MB from disk is much cheaper than waiting on
/// a slow network, so the UI shows cached models immediately and refreshes in the background.
pub fn cached() -> Option<Vec<CatalogModel>> {
    read_cached()
}

/// Whether the cached catalog is younger than the 24-hour TTL.
pub fn is_fresh() -> bool {
    let Ok(modified) = fs::metadata(cache_path()).and_then(|m| m.modified()) else { return false };
    SystemTime::now().duration_since(modified).map_or(false, |age| age < TTL)
}

fn fetch_and_cache(budget: Duration) -> Result<Vec<CatalogModel>, String> {
    // A hard total-request timeout matters: models.dev can trickle bytes for minutes, and ureq's
    // default has no read timeout at all.
    let agent = ureq::AgentBuilder::new()
        .timeout(budget)
        .timeout_connect(Duration::from_secs(5))
        .build();
    let response = agent.get(API)
        .set("User-Agent", "rivo")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let text = response.into_string().map_err(|e| format!("read failed: {e}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("invalid catalog: {e}"))?;
    let models = parse(&value);
    if models.is_empty() {
        return Err("catalog contained no models".into());
    }
    if let Some(parent) = cache_path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(cache_path(), &text);
    Ok(models)
}

/// First load: serve any cached copy at once; only a machine with no cache waits for the network.
pub fn load() -> Result<Vec<CatalogModel>, String> {
    if let Some(models) = cached() {
        return Ok(models);
    }
    fetch_and_cache(Duration::from_secs(25))
}

/// Background refresh with a strict time budget. On failure the caller keeps its cached copy.
pub fn refresh() -> Result<Vec<CatalogModel>, String> {
    fetch_and_cache(Duration::from_secs(10))
}

// ---- official provider logos ---------------------------------------------------------------
// models.dev ships one `logo.svg` per lab/provider in its public repository. The files are
// monochrome (`fill="currentColor"`), so Rune's SVG rasterizer would paint them black. We
// substitute each lab's brand colour before handing the bytes to the renderer.

/// Brand colour used for a lab's monochrome logo. Unknown labs get a light slate that stays
/// readable on Rivo's dark theme instead of disappearing into black.
pub fn logo_color(id: &str) -> &'static str {
    match id {
        // Values are the marks' own colours from public brand guidelines / trademark assets.
        "alibaba" => "#FF6A00",    // Alibaba orange
        "anthropic" => "#D97757",  // Anthropic "Clay" — the official mark fill
        "cohere" => "#FF7759",     // Cohere "Bittersweet" coral (mark colour)
        "deepseek" => "#4D6BFE",   // DeepSeek blue, from the official logo SVG
        "elevenlabs" => "#FFFFFF", // official white mark for dark backgrounds
        "google" => "#4285F4",     // Google blue
        "huawei" => "#CF0A2C",     // Huawei red
        "meta" => "#0866FF",       // Meta blue
        "mistral" => "#FF7000",    // Mistral logo orange
        "moonshotai" => "#FFFFFF", // Kimi's white mark on dark
        "nvidia" => "#76B900",     // NVIDIA green (trademark guidelines)
        "openai" => "#10A37F",     // OpenAI green accent; the Blossom itself is monochrome
        "perplexity" => "#20808D", // Perplexity "True Turquoise"
        "poolside" => "#4137FF",   // Poolside electric blue
        "tencent" => "#0052D9",    // Tencent blue
        "xai" => "#FFFFFF",        // xAI publishes black/white marks; white on dark
        "xiaomi" => "#FF6900",     // Xiaomi orange
        "zhipuai" => "#3859FF",    // Zhipu blue
        // Labs without a verifiable brand colour use the light fallback below.
        _ => "#C9D4E4",
    }
}

/// Replace `currentColor` with the lab's brand colour so the official mark is not painted black.
pub fn colorize_logo(id: &str, bytes: &[u8]) -> Vec<u8> {
    let svg = String::from_utf8_lossy(bytes);
    if !svg.contains("currentColor") {
        return bytes.to_vec();
    }
    svg.replace("currentColor", logo_color(id)).into_bytes()
}

fn logo_dir() -> PathBuf {
    cache_path().parent().unwrap_or_else(|| Path::new(".")).join("logos")
}

/// Provider ids come from the catalog JSON; never let one escape the logo cache folder.
fn safe_logo_id(id: &str) -> Option<&str> {
    let id = id.trim();
    if id.is_empty() || id.len() > 64 {
        return None;
    }
    if id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        Some(id)
    } else {
        None
    }
}

fn logo_path(id: &str) -> Option<PathBuf> {
    safe_logo_id(id).map(|id| logo_dir().join(format!("{id}.svg")))
}

fn cached_logo(id: &str) -> Option<Vec<u8>> {
    // A stale logo is still better than no logo: the network can be very slow, so never block a
    // row just because the 24h TTL elapsed. Missing logos are the only ones downloaded.
    let raw = fs::read(logo_path(id)?).ok()?;
    Some(colorize_logo(id, &raw))
}

/// Cached logo bytes without any network access. Used by headless scenes and tests.
pub fn cached_logo_bytes(id: &str) -> Option<Vec<u8>> {
    cached_logo(id)
}

/// Fetch one logo, trying the lab first and the API provider second. Both are official
/// models.dev assets; not every id has both.
fn fetch_logo(id: &str) -> Option<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(4))
        .timeout_connect(Duration::from_secs(2))
        .build();
    for kind in ["labs", "providers"] {
        let url = format!("{LOGO_BASE}/{kind}/{id}/logo.svg");
        let Ok(response) = agent.get(&url).set("User-Agent", "rivo").call() else { continue };
        if response.status() != 200 {
            continue;
        }
        let Ok(text) = response.into_string() else { continue };
        if !text.contains("<svg") || text.len() > 256 * 1024 {
            continue;
        }
        let bytes = text.into_bytes();
        if let Some(path) = logo_path(id) {
            let _ = fs::create_dir_all(logo_dir());
            let _ = fs::write(path, &bytes);
        }
        return Some(colorize_logo(id, &bytes));
    }
    None
}

/// Return the logo bytes for every provider id we can find. Fresh cached files are read from
/// disk; missing or stale ones are downloaded on a small worker pool. Safe to call repeatedly —
/// a second call within 24 hours only reads files.
pub fn ensure_logos(ids: &[String]) -> Vec<(String, Vec<u8>)> {
    let unique: HashSet<&str> = ids.iter().map(|s| s.as_str()).filter_map(safe_logo_id).collect();
    let mut todo: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for id in unique {
        match cached_logo(id) {
            Some(bytes) => out.push((id.to_string(), bytes)),
            None => todo.push(id.to_string()),
        }
    }
    if todo.is_empty() {
        return out;
    }
    let next = Arc::new(AtomicUsize::new(0));
    let todo = Arc::new(todo);
    let found = Arc::new(Mutex::new(Vec::new()));
    let workers = LOGO_WORKERS.min(todo.len());
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let (next, todo, found) = (Arc::clone(&next), Arc::clone(&todo), Arc::clone(&found));
        handles.push(thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= todo.len() {
                break;
            }
            let id = &todo[i];
            if let Some(bytes) = fetch_logo(id) {
                found.lock().map(|mut v| v.push((id.clone(), bytes))).ok();
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    if let Ok(mut found) = found.lock() {
        out.append(&mut found);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Filter and search. Rank prefix matches on the model name first, then provider/name contains.
pub fn filter<'a>(models: &'a [CatalogModel], query: &str, category: Category) -> Vec<&'a CatalogModel> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<_> = models.iter()
        .filter(|m| category.matches(m))
        .filter(|m| q.is_empty()
            || m.name.to_lowercase().contains(&q)
            || m.id.to_lowercase().contains(&q)
            || m.provider_name.to_lowercase().contains(&q)
            || m.provider.to_lowercase().contains(&q))
        .collect();
    out.sort_by_key(|m| {
        let name = m.name.to_lowercase();
        if name.starts_with(&q) { 0 } else if m.provider.to_lowercase().starts_with(&q) { 1 } else { 2 }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn protocol_and_base_url_mapping() {
        assert_eq!(protocol_for("@ai-sdk/anthropic"), Protocol::Anthropic);
        assert_eq!(protocol_for("@ai-sdk/google"), Protocol::Gemini);
        assert_eq!(protocol_for("@ai-sdk/openai-compatible"), Protocol::OpenAiCompatible);
        assert_eq!(base_url_for("@ai-sdk/openai", ""), "https://api.openai.com/v1");
        assert_eq!(base_url_for("@ai-sdk/openai-compatible", "https://example.com/v1"), "https://example.com/v1");
    }

    #[test]
    fn parses_provider_metadata_and_costs() {
        let root = json!({
            "example": {
                "name": "Example",
                "api": "https://example.com/v1",
                "npm": "@ai-sdk/openai-compatible",
                "env": ["EXAMPLE_API_KEY"],
                "models": {
                    "small": {
                        "name": "Small",
                        "description": "a small model",
                        "tool_call": true,
                        "reasoning": false,
                        "open_weights": true,
                        "limit": {"context": 128000, "output": 4096},
                        "cost": {"input": 0.0, "output": 0.0}
                    }
                }
            }
        });
        let models = parse(&root);
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!(m.base_url, "https://example.com/v1");
        assert_eq!(m.env, "EXAMPLE_API_KEY");
        assert_eq!(m.context, 128_000);
        assert!(m.free && m.tool_call && m.open_weights && !m.reasoning);
    }

    #[test]
    fn lab_comes_from_canonical_model_not_the_router() {
        let root = json!({
            "agentrouter": {
                "name": "AgentRouter",
                "api": "https://agentrouter.org/v1",
                "npm": "@ai-sdk/openai-compatible",
                "models": {
                    "claude-opus-5": {
                        "name": "Claude Opus 5",
                        "canonical_model_id": "anthropic/claude-opus-5",
                        "limit": {"context": 1000, "output": 100},
                        "cost": {"input": 0.0, "output": 0.0}
                    },
                    "standalone": {
                        "name": "Standalone",
                        "limit": {"context": 1000, "output": 100},
                        "cost": {"input": 0.0, "output": 0.0}
                    }
                }
            }
        });
        let models = parse(&root);
        let claude = models.iter().find(|m| m.id == "claude-opus-5").unwrap();
        assert_eq!(claude.provider, "agentrouter");
        assert_eq!(claude.lab, "anthropic", "logo must be Anthropic's, not the router's");
        let standalone = models.iter().find(|m| m.id == "standalone").unwrap();
        assert_eq!(standalone.lab, "agentrouter");
    }

    #[test]
    #[ignore = "uses the real on-disk catalog cache"]
    fn stale_cache_is_served_without_waiting_for_the_network() {
        let started = std::time::Instant::now();
        let models = load().expect("a cached catalog should always load");
        assert!(!models.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2), "cache read took {:?}", started.elapsed());
    }

    #[test]
    #[ignore = "downloads the real models.dev logo; run manually to verify caching"]
    fn downloads_and_caches_an_official_logo() {
        let ids = vec!["openai", "anthropic", "google", "deepseek", "xai", "meta", "alibaba", "cohere"]
            .into_iter().map(str::to_string).collect::<Vec<_>>();
        let first = ensure_logos(&ids);
        assert!(first.len() >= 2, "expected official SVGs, got {first:?}");
        assert!(first.iter().all(|(_, bytes)| bytes.windows(4).any(|w| w == b"<svg")));
        let second = ensure_logos(&ids);
        assert_eq!(second.len(), first.len(), "a warm cache must return the same logos");
    }

    #[test]
    fn logos_are_painted_in_the_labs_brand_colour() {
        let svg = br#"<svg><path fill="currentColor" d="M0 0"/></svg>"#;
        let colored = colorize_logo("openai", svg);
        let text = String::from_utf8(colored).unwrap();
        assert!(text.contains("#10A37F"), "OpenAI green missing: {text}");
        assert!(!text.contains("currentColor"));
        let anthropic = String::from_utf8(colorize_logo("anthropic", svg)).unwrap();
        assert!(anthropic.contains("#D97757"));
        let cohere = String::from_utf8(colorize_logo("cohere", svg)).unwrap();
        assert!(cohere.contains("#FF7759"), "Cohere's mark is coral, not purple");
        let xai = String::from_utf8(colorize_logo("xai", svg)).unwrap();
        assert!(xai.contains("#FFFFFF"), "xAI's dark-background mark is white");
        let unknown = String::from_utf8(colorize_logo("some-lab", svg)).unwrap();
        assert!(unknown.contains("#C9D4E4"), "fallback must not be black");
    }

    #[test]
    fn logo_ids_are_confined_to_the_cache_folder() {
        assert_eq!(safe_logo_id("openai"), Some("openai"));
        assert_eq!(safe_logo_id("-amazon-bedrock.2"), Some("-amazon-bedrock.2"));
        assert_eq!(safe_logo_id("../escape"), None);
        assert_eq!(safe_logo_id("has space"), None);
        assert_eq!(safe_logo_id(""), None);
        assert!(logo_path("openai").unwrap().ends_with("logos/openai.svg"));
    }

    #[test]
    fn category_free_filters_zero_cost_models() {
        let make = |cost: f64, name: &str| CatalogModel {
            id: name.into(), name: name.into(), description: String::new(),
            provider: "p".into(), provider_name: "P".into(), lab: "p".into(), base_url: String::new(),
            env: String::new(), protocol: Protocol::OpenAiCompatible, context: 1000,
            max_output: 100, input_cost: cost, output_cost: cost, free: cost == 0.0,
            tool_call: false, reasoning: false, open_weights: false,
        };
        let models = vec![make(0.0, "free"), make(1.0, "paid")];
        let found = filter(&models, "", Category::Free);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "free");
        assert_eq!(filter(&models, "paid", Category::All).len(), 1);
    }
}
