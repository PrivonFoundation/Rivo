//! Public, free registries for the Settings marketplaces.
//!
//! Sources (both HTTPS, no API key, no signup):
//! - MCP: the official MCP Registry `https://registry.modelcontextprotocol.io`, whose
//!   publishers verify a reverse-DNS namespace via GitHub OAuth or DNS/HTTP challenges.
//!   It hosts metadata only — the code lives in npm/PyPI/Docker and is not scanned here.
//! - Skills: `https://skills.sh` (operated by Vercel), which indexes public GitHub skills,
//!   shows install counts and partner audit results, and exposes a download endpoint.
//!
//! Nothing in this module executes downloaded code. Search and download happen on a
//! worker thread; installs only write text files into the user's config/workspace.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

const MCP_API: &str = "https://registry.modelcontextprotocol.io/v0/servers";
const SKILLS_API: &str = "https://skills.sh/api";

/// What installing an entry actually means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallSpec {
    /// A stdio server from a package registry, launched through a package runner.
    Command { program: String, args: Vec<String>, env: Vec<String> },
    /// A hosted server reached over HTTP/SSE.
    Remote { url: String },
    /// A skill whose files live in a public GitHub repo.
    Skill { source: String, skill_id: String },
}

/// One search result from either marketplace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub publisher: String,
    pub installs: u64,
    /// Namespace verified by the MCP Registry, or a first-party/curated skill source.
    pub verified: bool,
    pub install: InstallSpec,
}

fn get_json(url: &str) -> Result<Value, String> {
    let response = ureq::get(url)
        .set("User-Agent", "rivo")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    response.into_json().map_err(|e| format!("invalid response: {e}"))
}

fn as_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn mcp_install(server: &Value) -> InstallSpec {
    if let Some(url) = server.get("remotes").and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|r| r.get("url")).and_then(Value::as_str)
    {
        return InstallSpec::Remote { url: url.to_string() };
    }
    let Some(pkg) = server.get("packages").and_then(Value::as_array).and_then(|a| a.first()) else {
        return InstallSpec::Remote { url: String::new() };
    };
    let identifier = as_str(pkg, "identifier");
    let kind = as_str(pkg, "registryType");
    let (program, mut args) = match kind.as_str() {
        "pypi" => ("uvx".to_string(), vec![identifier.clone()]),
        "docker" => ("docker".to_string(), vec!["run".into(), "-i".into(), "--rm".into(), identifier.clone()]),
        _ => ("npx".to_string(), vec!["-y".into(), identifier.clone()]),
    };
    if let Some(extra) = pkg.get("runtimeArguments").and_then(Value::as_array) {
        for arg in extra.iter().filter_map(|a| a.get("value").and_then(Value::as_str)) {
            if !args.iter().any(|a| a == arg) {
                args.push(arg.to_string());
            }
        }
    }
    let env = pkg.get("environmentVariables").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|e| e.get("name").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default();
    InstallSpec::Command { program, args, env }
}

fn mcp_verified(name: &str) -> bool {
    // The registry requires GitHub OAuth or DNS/HTTP proof for these namespace forms.
    name.starts_with("io.github.") || name.starts_with("com.github.") || name.starts_with("me.")
}

/// Search the official MCP registry. Results are deduplicated by server name, keeping the
/// last (newest) version returned for that name.
pub fn search_mcp(query: &str, limit: usize) -> Result<Vec<RegistryItem>, String> {
    let url = format!("{MCP_API}?limit={}&search={}", limit.min(100).max(1), urlencode(query));
    let data = get_json(&url)?;
    let mut by_name: HashMap<String, RegistryItem> = HashMap::new();
    for entry in data.get("servers").and_then(Value::as_array).into_iter().flatten() {
        let server = &entry["server"];
        let name = as_str(server, "name");
        if name.is_empty() {
            continue;
        }
        let item = RegistryItem {
            id: name.clone(),
            name: as_str(server, "title"),
            description: as_str(server, "description"),
            publisher: name.clone(),
            installs: 0,
            verified: mcp_verified(&name),
            install: mcp_install(server),
        };
        let display = if item.name.is_empty() { name.clone() } else { item.name.clone() };
        let mut item = item;
        item.name = display;
        by_name.insert(name, item);
    }
    let mut out: Vec<_> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn parse_skill(v: &Value) -> Option<RegistryItem> {
    let id = as_str(v, "id");
    let source = as_str(v, "source");
    let skill_id = as_str(v, "skillId");
    let installs = v.get("installs").and_then(Value::as_u64).unwrap_or(0);
    if id.is_empty() || source.is_empty() || skill_id.is_empty() {
        return None;
    }
    let name = {
        let n = as_str(v, "name");
        if n.is_empty() { skill_id.clone() } else { n }
    };
    Some(RegistryItem {
        id: id.clone(),
        name,
        description: format!("{installs} installs · {source}"),
        publisher: source.clone(),
        installs,
        verified: is_first_party(&source),
        install: InstallSpec::Skill { source, skill_id },
    })
}

fn is_first_party(source: &str) -> bool {
    matches!(source, "anthropics/skills" | "vercel-labs/agent-skills" | "vercel-labs/skills")
}

/// Search skills.sh. The CLI's public `/api/search` endpoint needs no credentials.
pub fn search_skills(query: &str, limit: usize) -> Result<Vec<RegistryItem>, String> {
    let q = if query.trim().len() < 2 { "skills" } else { query.trim() };
    let url = format!("{SKILLS_API}/search?q={}&limit={}", urlencode(q), limit.min(50).max(1));
    let data = get_json(&url)?;
    let mut out: Vec<RegistryItem> = data.get("skills").and_then(Value::as_array)
        .into_iter().flatten().filter_map(parse_skill).collect();
    out.sort_by(|a, b| b.installs.cmp(&a.installs));
    Ok(out)
}

/// Download the text files of a skill. Files are limited in size and count; nothing is written.
pub fn download_skill(source: &str, skill_id: &str) -> Result<Vec<(String, String)>, String> {
    // `source` is `owner/repo` and must stay a two-segment URL path, not %2F.
    let url = format!("{SKILLS_API}/download/{source}/{}", urlencode(skill_id));
    let data = get_json(&url)?;
    let mut files = Vec::new();
    for file in data.get("files").and_then(Value::as_array).into_iter().flatten().take(32) {
        let path = as_str(file, "path");
        let contents = as_str(file, "contents");
        if safe_relative(&path) && !contents.is_empty() && contents.len() <= 256 * 1024 {
            files.push((path, contents));
        }
    }
    if files.is_empty() {
        return Err("no usable skill files were returned".into());
    }
    Ok(files)
}

fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"))
        .join("rivo")
}

/// Add or replace one MCP server in Rivo's own `~/.config/rivo/mcp.json`.
/// The shape matches `info::mcp_servers`, so the inspector sees it immediately.
pub fn install_mcp(item: &RegistryItem) -> Result<String, String> {
    let (name, entry) = match &item.install {
        InstallSpec::Command { program, args, env } => (
            item.id.clone(),
            serde_json::json!({ "command": program, "args": args, "env": env }),
        ),
        InstallSpec::Remote { url } if !url.is_empty() => (
            item.id.clone(),
            serde_json::json!({ "type": "remote", "url": url }),
        ),
        _ => return Err("this server has no supported launch method".into()),
    };
    if name.is_empty() {
        return Err("this server has no name".into());
    }
    let path = config_dir().join("mcp.json");
    let mut root: Value = std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "mcpServers": {} }));
    let servers = root["mcpServers"].as_object_mut().ok_or_else(|| "mcp.json is malformed".to_string())?;
    servers.insert(name.clone(), entry);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(format!("Added {name} to {}", path.display()))
}

/// Write a downloaded skill under `<workspace>/.rivo/skills/<slug>/`.
pub fn install_skill(workspace: &Path, item: &RegistryItem) -> Result<String, String> {
    let InstallSpec::Skill { source, skill_id } = &item.install else {
        return Err("not a skill entry".into());
    };
    let files = download_skill(source, skill_id)?;
    let target = workspace.join(".rivo").join("skills").join(skill_id);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    for (path, contents) in files {
        let dest = target.join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, contents).map_err(|e| e.to_string())?;
    }
    Ok(format!("Installed {} into {}", item.name, target.display()))
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_sanitized() {
        assert!(safe_relative("SKILL.md"));
        assert!(safe_relative("references/api.md"));
        assert!(!safe_relative("../escape.md"));
        assert!(!safe_relative("/etc/passwd"));
        assert!(!safe_relative("a/../../b"));
    }

    #[test]
    fn url_encoding_handles_spaces_and_unicode() {
        assert_eq!(urlencode("code review"), "code%20review");
        assert_eq!(urlencode("résumé"), "r%C3%A9sum%C3%A9");
    }

    #[test]
    fn registry_namespace_marks_verified_publishers() {
        assert!(mcp_verified("io.github.anthropic/mcp-server-git"));
        assert!(mcp_verified("me.acme.com/server"));
        assert!(!mcp_verified("random-server"));
    }
}
