//! Wire protocols. Rivo can talk to three families of endpoints with one tool/event model:
//! OpenAI-compatible chat completions, Anthropic Messages, and Google Gemini.

use std::collections::BTreeMap;
use std::io::Read;

use serde_json::{Value, json};

use crate::model::{Message, ProviderConfig, Role, TokenUsage, ToolCall};
use crate::settings::ModelConfig;
use crate::provider::StreamEvent;
use crate::tools::{Tool, openai_specs};

/// Build the system prompt, including the catalog of enabled skills (Claude-Code style).
pub fn system_prompt(run: &crate::settings::RunConfig) -> String {
    let mut s = String::from(
        "You are Rivo, a careful coding agent running as a native desktop app. \
You work in the user's workspace and have 50 tools for file management, search, git, \
allowlisted and background commands, web lookup and session notes. Prefer reading before \
writing. Keep changes small and focused. Explain what you did briefly when you are finished. \
Use Markdown in your answers.\n",
    );
    if !run.settings.system_prompt.trim().is_empty() {
        s.push_str("\n## User instructions\n\n");
        s.push_str(run.settings.system_prompt.trim());
        s.push('\n');
    }
    if !run.skills.is_empty() {
        s.push_str("\n## Available skills\n\nEach skill is a short playbook. Call `skill_load` with its name to read the full instructions when the task matches. Don't load one you don't need.\n\n");
        for skill in &run.skills {
            s.push_str(&format!("- **{}** — {}\n", skill.name, skill.description));
        }
    }
    s
}

pub fn chat(
    config: &ProviderConfig,
    model: &ModelConfig,
    run: &crate::settings::RunConfig,
    messages: &[Message],
    tools: &[Box<dyn Tool>],
    emit: &mut dyn FnMut(StreamEvent),
) -> Result<(), String> {
    match model.protocol {
        crate::model::Protocol::OpenAiCompatible => chat_openai(config, model, run, messages, tools, emit),
        crate::model::Protocol::Anthropic => chat_anthropic(config, model, run, messages, tools, emit),
        crate::model::Protocol::Gemini => chat_gemini(config, model, run, messages, tools, emit),
    }
}

fn auth_headers(config: &ProviderConfig, req: ureq::Request) -> ureq::Request {
    if config.api_key.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {}", config.api_key))
    }
}

fn post(url: &str, config: &ProviderConfig, extra: &[(&str, &str)], body: String) -> Result<ureq::Response, String> {
    let mut req = ureq::post(url).set("Content-Type", "application/json").set("Accept", "text/event-stream");
    req = auth_headers(config, req);
    for (k, v) in extra {
        req = req.set(k, v);
    }
    req.send_string(&body).map_err(|e| match e {
        ureq::Error::Status(code, response) => {
            let detail = response.into_string().unwrap_or_default();
            format!("HTTP {code}: {}", detail.chars().take(600).collect::<String>())
        }
        other => format!("request failed: {other}"),
    })
}

/// The shared SSE line pump: calls `line` for every `data:` payload until `[DONE]`/EOF.
fn sse(resp: ureq::Response, mut line: impl FnMut(&str) -> Result<bool, String>) -> Result<(), String> {
    let mut reader = resp.into_reader();
    let mut pending = String::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("stream read failed: {e}"))?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(pos) = pending.find('\n') {
            let raw: String = pending.drain(..=pos).collect();
            let raw = raw.trim_end_matches(['\n', '\r']);
            let Some(data) = raw.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                if data == "[DONE]" {
                    return Ok(());
                }
                continue;
            }
            if line(data)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------------------- OpenAI

fn openai_messages(run: &crate::settings::RunConfig, messages: &[Message]) -> Value {
    let mut out = vec![json!({"role": "system", "content": system_prompt(run)})];
    for m in messages {
        match m.role {
            Role::System => out.push(json!({"role": "system", "content": m.text})),
            Role::User => out.push(json!({"role": "user", "content": m.text})),
            Role::Assistant => {
                let calls: Vec<Value> = m
                    .tools
                    .iter()
                    .filter(|c| c.finished)
                    .map(|c| {
                        let args = serde_json::from_str::<Value>(&c.arguments).unwrap_or_else(|_| json!({}));
                        json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": args.to_string()}})
                    })
                    .collect();
                let mut msg = json!({"role": "assistant", "content": m.text});
                if !calls.is_empty() {
                    msg["tool_calls"] = Value::Array(calls);
                }
                out.push(msg);
                for c in m.tools.iter().filter(|c| c.finished) {
                    out.push(json!({"role": "tool", "tool_call_id": c.id, "content": c.result.clone().unwrap_or_default()}));
                }
            }
        }
    }
    Value::Array(out)
}

fn flush_calls(calls: &mut BTreeMap<usize, (String, String, String)>, emit: &mut dyn FnMut(StreamEvent)) {
    let pending = std::mem::take(calls);
    for (_, (id, name, args)) in pending {
        emit(StreamEvent::ToolCall { id, name, arguments: args });
    }
}

/// Copy a numeric usage field only when the provider actually reported it. Streaming
/// APIs send usage across several events, so we must not reset earlier counters.
fn take_u64(v: &Value, key: &str, dst: &mut u64) {
    if let Some(n) = v.get(key).and_then(Value::as_u64) {
        *dst = n;
    }
}

fn nested_u64(v: &Value, parent: &str, key: &str) -> u64 {
    v.get(parent).and_then(|p| p.get(key)).and_then(Value::as_u64).unwrap_or(0)
}

fn emit_usage(usage: TokenUsage, emit: &mut dyn FnMut(StreamEvent)) {
    if usage != TokenUsage::default() {
        emit(StreamEvent::Usage(usage));
    }
}

fn chat_openai(
    config: &ProviderConfig,
    model: &ModelConfig,
    run: &crate::settings::RunConfig,
    messages: &[Message],
    tools: &[Box<dyn Tool>],
    emit: &mut dyn FnMut(StreamEvent),
) -> Result<(), String> {
    let body = json!({
        "model": config.model,
        "stream": true,
        "temperature": model.temperature,
        "max_tokens": model.max_output,
        "messages": openai_messages(run, messages),
        "tools": openai_specs(tools),
    })
    .to_string();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let resp = post(&url, config, &[], body)?;
    let mut calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    let mut usage = TokenUsage::default();
    sse(resp, |data| {
        let v: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
        if let Some(err) = v.get("error") {
            return Err(format!("api error: {err}"));
        }
        if let Some(u) = v.get("usage") {
            let cached = nested_u64(u, "prompt_tokens_details", "cached_tokens");
            let mut prompt = 0u64;
            take_u64(u, "prompt_tokens", &mut prompt);
            usage.input = prompt.saturating_sub(cached);
            usage.cache_read = cached;
            take_u64(u, "completion_tokens", &mut usage.output);
            usage.reasoning = nested_u64(u, "completion_tokens_details", "reasoning_tokens");
        }
        let Some(choice) = v.get("choices").and_then(|c| c.as_array().and_then(|a| a.first())) else { return Ok(false) };
        let delta = &choice["delta"];
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                emit(StreamEvent::Delta(text.to_string()));
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
            for tc in tcs {
                let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let entry = calls.entry(index).or_insert_with(|| (String::new(), String::new(), String::new()));
                if let Some(id) = tc.get("id").and_then(Value::as_str) {
                    if !id.is_empty() { entry.0 = id.to_string(); }
                }
                if let Some(f) = tc.get("function") {
                    if let Some(name) = f.get("name").and_then(Value::as_str) {
                        if !name.is_empty() { entry.1 = name.to_string(); }
                    }
                    if let Some(args) = f.get("arguments").and_then(Value::as_str) {
                        entry.2.push_str(args);
                    }
                }
            }
        }
        Ok(false)
    })?;
    flush_calls(&mut calls, emit);
    emit_usage(usage, emit);
    emit(StreamEvent::Done);
    Ok(())
}

// ------------------------------------------------------------------------- Anthropic

fn anthropic_messages(messages: &[Message]) -> Value {
    let mut out = Vec::new();
    for m in messages {
        match m.role {
            Role::User => out.push(json!({"role": "user", "content": m.text})),
            Role::Assistant => {
                let mut blocks: Vec<Value> = Vec::new();
                if !m.text.is_empty() {
                    blocks.push(json!({"type": "text", "text": m.text}));
                }
                for c in m.tools.iter().filter(|c| c.finished) {
                    blocks.push(json!({
                        "type": "tool_use", "id": c.id, "name": c.name,
                        "input": serde_json::from_str::<Value>(&c.arguments).unwrap_or_else(|_| json!({}))
                    }));
                }
                if !blocks.is_empty() {
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            Role::System => {}
        }
        // Tool results are a user message containing tool_result blocks.
        let results: Vec<&ToolCall> = m.tools.iter().filter(|c| c.finished && m.role == Role::Assistant).collect();
        if !results.is_empty() && m.role == Role::Assistant {
            out.push(json!({
                "role": "user",
                "content": results.iter().map(|c| json!({
                    "type": "tool_result", "tool_call_id": c.id,
                    "content": c.result.clone().unwrap_or_default()
                })).collect::<Vec<_>>()
            }));
        }
    }
    Value::Array(out)
}

fn chat_anthropic(
    config: &ProviderConfig,
    model: &ModelConfig,
    run: &crate::settings::RunConfig,
    messages: &[Message],
    tools: &[Box<dyn Tool>],
    emit: &mut dyn FnMut(StreamEvent),
) -> Result<(), String> {
    let tool_defs: Vec<Value> = tools
        .iter()
        .map(|t| json!({"name": t.name(), "description": t.description(), "input_schema": t.schema()}))
        .collect();
    let body = json!({
        "model": config.model,
        "max_tokens": model.max_output,
        "temperature": model.temperature,
        "system": system_prompt(run),
        "messages": anthropic_messages(messages),
        "tools": tool_defs,
        "stream": true,
    })
    .to_string();
    let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
    let mut req = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream")
        .set("anthropic-version", "2023-06-01");
    if !config.api_key.is_empty() {
        req = req.set("x-api-key", &config.api_key);
    }
    let resp = req.send_string(&body).map_err(|e| format!("anthropic request failed: {e}"))?;

    // index -> (id, name, accumulated json)
    let mut calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    let mut usage = TokenUsage::default();
    sse(resp, |data| {
        let v: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
        if let Some(err) = v.get("error") {
            return Err(format!("api error: {err}"));
        }
        if let Some(u) = v.get("usage").or_else(|| v.get("message").and_then(|m| m.get("usage"))) {
            take_u64(u, "input_tokens", &mut usage.input);
            take_u64(u, "output_tokens", &mut usage.output);
            take_u64(u, "cache_read_input_tokens", &mut usage.cache_read);
            take_u64(u, "cache_creation_input_tokens", &mut usage.cache_write);
        }
        match v.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                if let Some(block) = v.get("content_block") {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let idx = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        calls.insert(idx, (id, name, String::new()));
                    }
                }
            }
            Some("content_block_delta") => {
                let delta = &v["delta"];
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str) {
                            emit(StreamEvent::Delta(text.to_string()));
                        }
                    }
                    Some("input_json_delta") => {
                        let idx = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        if let Some(entry) = calls.get_mut(&idx) {
                            if let Some(part) = delta.get("partial_json").and_then(Value::as_str) {
                                entry.2.push_str(part);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some("message_stop") => {
                flush_calls(&mut calls, emit);
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    })?;
    flush_calls(&mut calls, emit);
    emit_usage(usage, emit);
    emit(StreamEvent::Done);
    Ok(())
}

// ---------------------------------------------------------------------------- Gemini

fn gemini_contents(messages: &[Message]) -> Value {
    let mut out = Vec::new();
    for m in messages {
        let role = if m.role == Role::User { "user" } else { "model" };
        let mut parts: Vec<Value> = Vec::new();
        if !m.text.is_empty() {
            parts.push(json!({"text": m.text}));
        }
        for c in m.tools.iter().filter(|c| c.finished) {
            let args = serde_json::from_str::<Value>(&c.arguments).unwrap_or_else(|_| json!({}));
            parts.push(json!({"functionCall": {"name": c.name, "args": args}}));
        }
        // A tool result is a `functionResponse` part in a user turn.
        if m.role == Role::Assistant {
            let results: Vec<Value> = m
                .tools
                .iter()
                .filter(|c| c.finished)
                .map(|c| json!({"functionResponse": {"name": c.name, "response": {"result": c.result.clone().unwrap_or_default()}}}))
                .collect();
            if !results.is_empty() {
                out.push(json!({"role": "user", "parts": results}));
                continue;
            }
        }
        if !parts.is_empty() {
            out.push(json!({"role": role, "parts": parts}));
        }
    }
    Value::Array(out)
}

fn chat_gemini(
    config: &ProviderConfig,
    model: &ModelConfig,
    run: &crate::settings::RunConfig,
    messages: &[Message],
    tools: &[Box<dyn Tool>],
    emit: &mut dyn FnMut(StreamEvent),
) -> Result<(), String> {
    let declarations: Vec<Value> = tools
        .iter()
        .map(|t| json!({"name": t.name(), "description": t.description(), "parameters": t.schema()}))
        .collect();
    let body = json!({
        "system_instruction": {"parts": [{"text": system_prompt(run)}]},
        "contents": gemini_contents(messages),
        "tools": [{"function_declarations": declarations}],
        "generationConfig": {"temperature": model.temperature, "maxOutputTokens": model.max_output},
    })
    .to_string();
    let mut url = format!(
        "{}/models/{}:streamGenerateContent?alt=sse",
        config.base_url.trim_end_matches('/'),
        config.model
    );
    if !config.api_key.is_empty() {
        url.push_str(&format!("&key={}", config.api_key));
    }
    let resp = ureq::post(&url).set("Content-Type", "application/json").send_string(&body)
        .map_err(|e| format!("gemini request failed: {e}"))?;
    let mut counter = 0usize;
    let mut usage = TokenUsage::default();
    sse(resp, |data| {
        let v: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
        if let Some(err) = v.get("error") {
            return Err(format!("api error: {err}"));
        }
        if let Some(u) = v.get("usageMetadata") {
            let mut cached = 0u64;
            let mut prompt = 0u64;
            take_u64(u, "cachedContentTokenCount", &mut cached);
            take_u64(u, "promptTokenCount", &mut prompt);
            usage.input = prompt.saturating_sub(cached);
            usage.cache_read = cached;
            take_u64(u, "candidatesTokenCount", &mut usage.output);
            take_u64(u, "thoughtsTokenCount", &mut usage.reasoning);
        }
        let Some(candidate) = v.get("candidates").and_then(|c| c.as_array().and_then(|a| a.first())) else { return Ok(false) };
        if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(Value::as_array) {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        emit(StreamEvent::Delta(text.to_string()));
                    }
                }
                if let Some(call) = part.get("functionCall") {
                    let name = call.get("name").and_then(Value::as_str).unwrap_or("unknown").to_string();
                    let args = call.get("args").cloned().unwrap_or_else(|| json!({})).to_string();
                    counter += 1;
                    emit(StreamEvent::ToolCall { id: format!("gemini-{counter}"), name, arguments: args });
                }
            }
        }
        Ok(false)
    })?;
    emit_usage(usage, emit);
    emit(StreamEvent::Done);
    Ok(())
}
