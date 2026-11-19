//! Read-only facts for the inspector's info centre: git working-tree changes, configured
//! MCP servers and a small price table for cost estimates.
//!
//! Everything here touches the filesystem or spawns a process, so callers must run it
//! off the UI thread (the app refreshes it from `poll` on a worker thread).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::model::TokenUsage;

/// One changed file in the current workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifiedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// An MCP server discovered in one of the supported config files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub status: String,
}

fn git(workspace: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(workspace).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn numstat(workspace: &Path, args: &[&str], into: &mut HashMap<String, (usize, usize)>) {
    let Some(text) = git(workspace, args) else { return };
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if add == "-" || del == "-" {
            continue;
        }
        let (Ok(added), Ok(removed)) = (add.parse::<usize>(), del.parse::<usize>()) else { continue };
        // Rename entries are reported as "old => new"; keep the destination.
        let path = path.split_once(" => ").map(|(_, new)| new).unwrap_or(path).to_string();
        let slot = into.entry(path).or_insert((0, 0));
        slot.0 += added;
        slot.1 += removed;
    }
}

fn count_lines(path: &Path) -> usize {
    const MAX: u64 = 2 * 1024 * 1024;
    let Ok(meta) = std::fs::metadata(path) else { return 0 };
    if meta.len() > MAX {
        return 0;
    }
    std::fs::read(path).map_or(0, |bytes| {
        bytes.iter().filter(|b| **b == b'\n').count() + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"))
    })
}

fn untracked_files(workspace: &Path, dir: &str, out: &mut Vec<ModifiedFile>) {
    let mut stack = vec![workspace.join(dir.trim_end_matches('/'))];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            if out.len() >= 64 {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let rel = path.strip_prefix(workspace).unwrap_or(&path);
                out.push(ModifiedFile {
                    path: rel.to_string_lossy().into_owned(),
                    added: count_lines(&path),
                    removed: 0,
                });
            }
        }
    }
}

/// `git status` plus `git diff --numstat`, including untracked files.
pub fn modified_files(workspace: &Path) -> Vec<ModifiedFile> {
    let Some(status) = git(workspace, &["status", "--porcelain"]) else { return Vec::new() };
    let mut stats = HashMap::new();
    numstat(workspace, &["diff", "--numstat"], &mut stats);
    numstat(workspace, &["diff", "--cached", "--numstat"], &mut stats);

    let mut files: Vec<ModifiedFile> = Vec::new();
    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[..2];
        let mut path = line[3..].trim().to_string();
        if let Some((_, new)) = path.split_once(" -> ") {
            path = new.to_string();
        }
        if code.contains('?') && path.ends_with('/') {
            untracked_files(workspace, &path, &mut files);
            continue;
        }
        let (added, removed) = stats.remove(&path).unwrap_or((0, 0));
        let added = if code.contains('?') { count_lines(&workspace.join(&path)) } else { added };
        files.push(ModifiedFile { path, added, removed });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.truncate(64);
    files
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn command_exists(cmd: &str) -> bool {
    if Path::new(cmd).exists() {
        return true;
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(cmd).exists())
    })
}

fn server_status(def: &Value) -> String {
    let command = def.get("command").and_then(Value::as_str).or_else(|| {
        def.get("command").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str)
    });
    if let Some(cmd) = command {
        return if command_exists(cmd) { "Configured".into() } else { "Unavailable".into() };
    }
    if def.get("url").and_then(Value::as_str).is_some() {
        return "Remote configured".into();
    }
    "Configured".into()
}

fn collect(obj: Option<&serde_json::Map<String, Value>>, out: &mut Vec<McpServer>) {
    let Some(obj) = obj else { return };
    for (name, def) in obj {
        if out.iter().any(|s| s.name == *name) {
            continue;
        }
        let enabled = def.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        out.push(McpServer {
            name: name.clone(),
            status: if enabled { server_status(def) } else { "Disabled".into() },
        });
    }
}

/// Read MCP server definitions from Rivo's config, the workspace and — as a convenience —
/// the OpenCode config, which uses the same shape under `mcp.servers`.
pub fn mcp_servers(workspace: &Path) -> Vec<McpServer> {
    let paths = [
        home_dir().join(".config/rivo/mcp.json"),
        workspace.join(".mcp.json"),
        workspace.join(".rivo/mcp.json"),
        home_dir().join(".config/opencode/opencode.json"),
    ];
    let mut out = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        collect(v.get("mcpServers").and_then(Value::as_object), &mut out);
        collect(v.get("mcp").and_then(|m| m.get("servers")).and_then(Value::as_object), &mut out);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// USD estimate per million tokens for a handful of paid models. Everything in Rivo's
/// free catalog (and the local mock) costs nothing.
pub fn cost_usd(model: &str, u: &TokenUsage) -> f64 {
    if model.contains("free") || model.starts_with("mock") {
        return 0.0;
    }
    let m = model.to_lowercase();
    let (input, output, cached) = if m.contains("gpt-4o-mini") {
        (0.15, 0.60, 0.075)
    } else if m.contains("gpt-4o") {
        (2.50, 10.00, 1.25)
    } else if m.contains("o3-mini") || m.contains("o4-mini") {
        (1.10, 4.40, 0.55)
    } else if m.contains("claude") {
        (3.00, 15.00, 0.30)
    } else {
        (0.0, 0.0, 0.0)
    };
    (u.input as f64 * input + u.cache_read as f64 * cached + (u.output + u.reasoning) as f64 * output) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn cache_rate_counts_only_input_and_cache_read() {
        let u = TokenUsage { input: 2_036_366, cache_read: 54_790_400, ..Default::default() };
        assert!((u.cache_rate() - 96.4).abs() < 0.1);
    }

    #[test]
    fn free_models_cost_nothing() {
        let u = TokenUsage { input: 10_000, output: 2_000, ..Default::default() };
        assert_eq!(cost_usd("kilo-auto/free", &u), 0.0);
    }

    #[test]
    fn collects_mcp_servers_from_config() {
        let v = json!({
            "mcp": { "servers": {
                "blender": { "type": "local", "command": ["/bin/true"] },
                "broken": { "command": ["/definitely/not/a/binary"] }
            } }
        });
        let mut out = Vec::new();
        collect(v.get("mcp").and_then(|m| m.get("servers")).and_then(Value::as_object), &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|s| s.name == "blender" && s.status == "Configured"));
        assert!(out.iter().any(|s| s.name == "broken" && s.status == "Unavailable"));
    }
}
