//! Provider dispatch. The wire details live in `protocol.rs`; here we only choose between the
//! local deterministic provider and a remote endpoint, and probe a connection for settings.

use std::thread;
use std::time::Duration;

use crate::model::{Message, Preset, ProviderConfig, Role, TokenUsage};
use crate::protocol;
use crate::settings::RunConfig;

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Delta(String),
    ToolCall { id: String, name: String, arguments: String },
    /// Token counters from the provider's final usage object, if it sends one.
    Usage(TokenUsage),
    Done,
}

pub trait Provider: Send + Sync {
    fn chat(&self, run: &RunConfig, messages: &[Message], emit: &mut dyn FnMut(StreamEvent)) -> Result<(), String>;
}

/// A lightweight connection check used by the settings screen. Runs on a worker thread.
pub fn probe(config: &ProviderConfig, protocol: crate::model::Protocol) -> Result<String, String> {
    if let Some(problem) = config.problem() {
        return Err(problem.into());
    }
    if config.preset == Preset::Mock {
        return Ok("Mock provider is local — no connection needed".into());
    }
    let base = config.base_url.trim_end_matches('/');
    let url = match protocol {
        crate::model::Protocol::OpenAiCompatible => format!("{base}/models"),
        crate::model::Protocol::Gemini => {
            let mut u = format!("{base}/models");
            if !config.api_key.is_empty() { u.push_str(&format!("?key={}", config.api_key)); }
            u
        }
        crate::model::Protocol::Anthropic => base.to_string(),
    };
    let mut req = ureq::get(&url);
    if !config.api_key.is_empty() && protocol != crate::model::Protocol::Gemini {
        req = match protocol {
            crate::model::Protocol::Anthropic => req.set("x-api-key", &config.api_key).set("anthropic-version", "2023-06-01"),
            _ => req.set("Authorization", &format!("Bearer {}", config.api_key)),
        };
    }
    // Any HTTP answer means the endpoint is reachable; 401/403 means the key is wrong.
    match req.call() {
        Ok(response) => {
            let status = response.status();
            let body: serde_json::Value = response.into_json().unwrap_or_default();
            let count = body.get("data").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
            if status == 200 && count > 0 {
                Ok(format!("Connected — {count} models available"))
            } else {
                Ok(format!("Endpoint reachable (HTTP {status})"))
            }
        }
        Err(ureq::Error::Status(code, _)) if code == 401 || code == 403 => {
            Err(format!("HTTP {code}: check the API key"))
        }
        Err(ureq::Error::Status(code, _)) => Ok(format!("Endpoint reachable (HTTP {code})")),
        Err(e) => Err(format!("request failed: {e}")),
    }
}

pub fn make(run: &RunConfig) -> Box<dyn Provider> {
    match run.provider.kind {
        crate::model::ProviderKind::Mock => Box::new(MockProvider),
        crate::model::ProviderKind::OpenAiCompatible => Box::new(RemoteProvider),
    }
}

struct RemoteProvider;
impl Provider for RemoteProvider {
    fn chat(&self, run: &RunConfig, messages: &[Message], emit: &mut dyn FnMut(StreamEvent)) -> Result<(), String> {
        let tools = crate::tools::all_tools();
        protocol::chat(&run.provider, &run.model, run, messages, &tools, emit)
    }
}

/// A local, deterministic provider. It streams word by word and emits real tool calls,
/// so every screen of the app can be exercised without an API key or a network.
struct MockProvider;

impl Provider for MockProvider {
    fn chat(&self, _run: &RunConfig, messages: &[Message], emit: &mut dyn FnMut(StreamEvent)) -> Result<(), String> {
        let last_user = messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.text.clone()).unwrap_or_default();
        let lower = last_user.to_lowercase();
        let last_was_tools = messages.last().is_some_and(|m| {
            m.role == Role::Assistant && !m.tools.is_empty() && m.tools.iter().all(|t| t.finished)
        });

        let text = if last_was_tools {
            "Done — the tool results are attached above. Anything else you'd like me to change?".to_string()
        } else if lower.contains("list") || lower.contains("file") || lower.contains("structur") || lower.contains("fișier") {
            "I'll map the workspace first, then we can pick a file to work on.".to_string()
        } else if lower.contains("search") || lower.contains("grep") || lower.contains("caut") {
            "Let me look for that across the workspace.".to_string()
        } else if lower.contains("read") || lower.contains("show") || lower.contains("citesc") {
            "Opening the file now.".to_string()
        } else if lower.contains("write") || lower.contains("create") || lower.contains("scrie") {
            "I'll create that file in the demo sandbox, then show you the result.".to_string()
        } else if lower.contains("run") || lower.contains("test") || lower.contains("build") || lower.contains("ruleaz") {
            "Running a read-only command in the workspace.".to_string()
        } else {
            format!(
                "I'm **Rivo**, your native coding agent. I have {} tools and skills for code review, Rust and testing.\n\nTry: *list the files*, *search for `fn view`*, *read Cargo.toml*, *git status*, *load the code-review skill*, or *write a note*. Your message was: “{}”.",
                crate::tools::all_tools().len(),
                last_user
            )
        };

        let tool: Option<(String, String)> = if last_was_tools {
            None
        } else if lower.contains("list") || lower.contains("file") || lower.contains("structur") || lower.contains("fișier") {
            Some(("list_dir".into(), r#"{"path":"."}"#.into()))
        } else if lower.contains("search") || lower.contains("grep") || lower.contains("caut") {
            let q = quoted(&last_user).unwrap_or_else(|| "fn ".to_string());
            Some(("search".into(), serde_json::json!({"query": q}).to_string()))
        } else if lower.contains("skill") {
            Some(("skill_load".into(), r#"{"name":"code-review"}"#.into()))
        } else if lower.contains("write") || lower.contains("create") || lower.contains("scrie") {
            let name = quoted(&last_user).unwrap_or_else(|| "rivo-demo/note.md".to_string());
            Some(("write_file".into(), serde_json::json!({
                "path": name,
                "content": "# Rivo demo\n\nThis file was written by the agent's real `write_file` tool.\n"
            }).to_string()))
        } else if lower.contains("git") {
            Some(("git_status".into(), "{}".into()))
        } else if lower.contains("run") || lower.contains("test") || lower.contains("build") || lower.contains("ruleaz") {
            Some(("run_command".into(), r#"{"command":"pwd"}"#.into()))
        } else if lower.contains("read") || lower.contains("show") || lower.contains("citesc") {
            Some(("read_file".into(), r#"{"path":"Cargo.toml"}"#.into()))
        } else {
            None
        };

        for word in text.split_inclusive(' ') {
            emit(StreamEvent::Delta(word.to_string()));
            thread::sleep(Duration::from_millis(12));
        }
        if let Some((name, arguments)) = tool {
            emit(StreamEvent::ToolCall { id: format!("mock-{}", messages.len()), name, arguments });
        }
        // Synthetic counters so the inspector's usage/cost widgets are exercised offline.
        let mut input = (messages.iter().map(|m| m.text.len()).sum::<usize>() / 4) as u64;
        let cache_read = input * 4 / 5;
        input -= cache_read;
        emit(StreamEvent::Usage(TokenUsage {
            input,
            output: (text.len() / 4) as u64,
            reasoning: 0,
            cache_read,
            cache_write: 0,
        }));
        emit(StreamEvent::Done);
        Ok(())
    }
}

fn quoted(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if matches!(*b, b'`' | b'"' | b'\'') {
            let quote = *b;
            let rest = &s[i + 1..];
            if let Some(end) = rest.find(quote as char) {
                let found = &rest[..end];
                if !found.is_empty() { return Some(found.to_string()); }
            }
        }
    }
    None
}
