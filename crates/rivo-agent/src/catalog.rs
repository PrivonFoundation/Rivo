//! models.dev catalog: a free, open-source database of 300+ providers and 10k models.
//!
//! Data comes from `https://models.dev/api.json` (no API key). The full file is ~7 MB, so it
//! is fetched on a worker thread and cached at `~/.config/rivo/models-dev.json` for 24 hours.
//! A stale cache is used if the network is unavailable.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::model::Protocol;

const API: &str = "https://models.dev/api.json";
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// A model with everything Rivo needs to configure a provider call.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: String,
    pub provider_name: String,
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
            out.push(CatalogModel {
                id: model_id.clone(),
                name,
                description: str_field(model, "description"),
                provider: id.clone(),
                provider_name: provider_name.clone(),
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

/// Load the catalog from a fresh cache, or fetch and cache it. Works offline with a stale cache.
pub fn load() -> Result<Vec<CatalogModel>, String> {
    if cache_fresh() {
        if let Some(models) = read_cached() {
            return Ok(models);
        }
    }
    let response = ureq::get(API)
        .set("User-Agent", "rivo")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let text = response.into_string().map_err(|e| format!("read failed: {e}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("invalid catalog: {e}"))?;
    let models = parse(&value);
    if let Some(parent) = cache_path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(cache_path(), &text);
    Ok(models)
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
    fn category_free_filters_zero_cost_models() {
        let make = |cost: f64, name: &str| CatalogModel {
            id: name.into(), name: name.into(), description: String::new(),
            provider: "p".into(), provider_name: "P".into(), base_url: String::new(),
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
