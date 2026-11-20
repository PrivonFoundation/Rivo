//! The agent's tools. Everything is confined to the session's workspace root: no path
//! escapes it and `run_command` only allows a small, non-destructive allowlist.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::settings::PermissionMode;
use crate::skills::Skill;

pub struct ToolContext {
    pub workspace: PathBuf,
    pub permission: PermissionMode,
    pub skills: Vec<Skill>,
}

impl ToolContext {
    pub fn new(workspace: PathBuf, permission: PermissionMode, skills: Vec<Skill>) -> Self {
        Self { workspace, permission, skills }
    }

    pub(crate) fn writable(&self) -> Result<(), String> {
        match self.permission {
            PermissionMode::Auto => Ok(()),
            PermissionMode::ReadOnly => Err("read-only mode: switch to Auto in Settings to write files or run commands".into()),
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> Value;
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String>;
}

pub fn all_tools() -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ListDir),
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(Search),
        Box::new(Glob),
        Box::new(EditFile),
        Box::new(GitStatus),
        Box::new(GitDiff),
        Box::new(GitLog),
        Box::new(WebFetch),
        Box::new(SkillLoad),
        Box::new(TodoWrite),
        Box::new(MemoryWrite),
        Box::new(MemoryRead),
        Box::new(RunCommand),
    ];
    tools.extend(crate::tools_extra::all());
    tools
}

pub fn openai_specs(tools: &[Box<dyn Tool>]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.schema(),
                    }
                })
            })
            .collect(),
    )
}

/// Resolve a relative path inside the workspace, rejecting `..`, absolute paths and symlinks
/// that point out of the workspace.
pub(crate) fn safe_path(ctx: &ToolContext, raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() {
        return Err("path is empty".into());
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        return Err("absolute paths are not allowed; use a path relative to the workspace".into());
    }
    let root = ctx
        .workspace
        .canonicalize()
        .map_err(|e| format!("cannot resolve workspace: {e}"))?;
    let mut out = root.clone();
    for component in p.components() {
        match component {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
                if !out.starts_with(&root) {
                    return Err("path escapes the workspace".into());
                }
            }
            _ => return Err("unsupported path".into()),
        }
    }
    // Resolve symlinks when the target exists, then re-check confinement.
    if out.exists() {
        let canon = out.canonicalize().map_err(|e| e.to_string())?;
        if !canon.starts_with(&root) {
            return Err("path escapes the workspace (symlink)".into());
        }
        return Ok(canon);
    }
    Ok(out)
}

pub(crate) fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

pub(crate) fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}\n… [truncated]")
}

struct ListDir;
impl Tool for ListDir {
    fn name(&self) -> &'static str { "list_dir" }
    fn description(&self) -> &'static str { "List files and folders in the workspace. Relative paths only." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Relative directory, default '.'"}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let dir = safe_path(ctx, raw)?;
        let mut out = String::new();
        walk(&dir, &dir, 0, 3, &mut out);
        Ok(if out.is_empty() { "(empty directory)".into() } else { truncate(out, 12000) })
    }
}

fn walk(root: &Path, dir: &Path, depth: usize, max: usize, out: &mut String) {
    if depth > max {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "dist" | "build") {
            continue;
        }
        let rel = e.path().strip_prefix(root).unwrap_or(&e.path()).to_string_lossy().into_owned();
        if e.path().is_dir() {
            out.push_str(&format!("  {rel}/\n"));
            walk(root, &e.path(), depth + 1, max, out);
        } else {
            let size = fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("  {rel}  ({size} bytes)\n"));
        }
    }
}

struct ReadFile;
impl Tool for ReadFile {
    fn name(&self) -> &'static str { "read_file" }
    fn description(&self) -> &'static str { "Read a UTF-8 text file from the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Relative file path"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let bytes = fs::read(&path).map_err(|e| format!("read failed: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(truncate(text.into_owned(), 32000))
    }
}

struct WriteFile;
impl Tool for WriteFile {
    fn name(&self) -> &'static str { "write_file" }
    fn description(&self) -> &'static str { "Create or overwrite a text file inside the workspace. Parent directories are created." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string","description":"Relative file path"},
            "content":{"type":"string","description":"Full new content"}
        },"required":["path","content"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let content = arg_str(args, "content")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create dir failed: {e}"))?;
        }
        // Re-check the parent after creating directories: it may have been a symlink.
        let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
        let parent = path.parent().unwrap_or(&root).canonicalize().map_err(|e| e.to_string())?;
        if !parent.starts_with(&root) {
            return Err("path escapes the workspace".into());
        }
        let bytes = content.len();
        fs::write(&path, content).map_err(|e| format!("write failed: {e}"))?;
        let rel = path.strip_prefix(ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone()))
            .unwrap_or(&path).display();
        Ok(format!("wrote {bytes} bytes to {rel}"))
    }
}

struct Search;
impl Tool for Search {
    fn name(&self) -> &'static str { "search" }
    fn description(&self) -> &'static str { "Search workspace text files for a literal query. Returns file:line matches." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string","description":"Literal text to find"},
            "path":{"type":"string","description":"Optional relative subdirectory"}
        },"required":["query"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let query = arg_str(args, "query")?;
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let start = safe_path(ctx, raw)?;
        let root = ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone());
        let mut files = Vec::new();
        collect_files(&start, &mut files, 0);
        let mut out = String::new();
        let mut hits = 0usize;
        for f in files {
            let Ok(bytes) = fs::read(&f) else { continue };
            if bytes.contains(&0) || bytes.len() > 2_000_000 {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            for (i, line) in text.lines().enumerate() {
                if line.contains(&query) {
                    let rel = f.strip_prefix(&root).unwrap_or(&f).display();
                    out.push_str(&format!("{rel}:{}: {}\n", i + 1, line.trim()));
                    hits += 1;
                    if hits >= 60 {
                        out.push_str("… [more matches omitted]\n");
                        return Ok(out);
                    }
                }
            }
        }
        Ok(if out.is_empty() { format!("no matches for {query:?}") } else { out })
    }
}

pub(crate) fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "dist" | "build") {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            collect_files(&p, out, depth + 1);
        } else {
            out.push(p);
        }
    }
}

/// Parse and validate a command line: no shell metacharacters, allowlisted program.
pub(crate) fn command_parts(command: &str) -> Result<Vec<String>, String> {
    let bad = [';', '&', '|', '>', '<', '$', '`', '\n', '\r', '(', ')', '{', '}', '\\'];
    if command.chars().any(|c| bad.contains(&c)) {
        return Err("shell metacharacters are not allowed".into());
    }
    let parts: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    let Some(program) = parts.first() else { return Err("empty command".into()) };
    const ALLOW: &[&str] = &[
        "cargo", "git", "ls", "pwd", "cat", "rg", "find", "rustc", "node", "python3", "uname", "echo", "printf",
    ];
    if !ALLOW.contains(&program.as_str()) {
        return Err(format!("{program} is not on the allowlist"));
    }
    Ok(parts)
}

struct RunCommand;
impl Tool for RunCommand {
    fn name(&self) -> &'static str { "run_command" }
    fn description(&self) -> &'static str {
        "Run a read-only allowlisted command in the workspace (cargo, git, ls, pwd, cat, rg, find, rustc, node, python3, uname, echo)."
    }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "command":{"type":"string","description":"Command line without shell metacharacters"}
        },"required":["command"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let command = arg_str(args, "command")?;
        let parts = command_parts(&command)?;
        let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
        let mut child = Command::new(&parts[0])
            .args(&parts[1..])
            .current_dir(&root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn failed: {e}"))?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() > Duration::from_secs(30) => {
                    let _ = child.kill();
                    return Err("command timed out after 30s".into());
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(format!("wait failed: {e}")),
            }
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let mut s = String::new();
        s.push_str(&String::from_utf8_lossy(&out.stdout));
        if !out.stderr.is_empty() {
            s.push_str("\n[stderr]\n");
            s.push_str(&String::from_utf8_lossy(&out.stderr));
        }
        if !out.status.success() {
            s.push_str(&format!("\n[exit {}]", out.status));
        }
        Ok(truncate(s, 12000))
    }
}

struct Glob;
impl Tool for Glob {
    fn name(&self) -> &'static str { "glob" }
    fn description(&self) -> &'static str { "Find workspace files by glob pattern (`src/**/*.rs`, `*.toml`)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"pattern":{"type":"string"}},"required":["pattern"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let pattern = arg_str(args, "pattern")?;
        let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
        let mut files = Vec::new();
        collect_files(&root, &mut files, 0);
        let mut out = String::new();
        for f in files {
            let rel = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            if glob_match(&pattern, &rel) {
                out.push_str(&rel);
                out.push('\n');
            }
        }
        Ok(if out.is_empty() { format!("no files match {pattern:?}") } else { truncate(out, 8000) })
    }
}

/// Small glob matcher: `*` doesn't cross `/`, `**` does, `?` matches one char.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() { return t.is_empty(); }
        if p[0] == b'*' {
            let double = p.get(1) == Some(&b'*');
            // `**/` also matches zero directories.
            if double && p.get(2) == Some(&b'/') && go(&p[3..], t) {
                return true;
            }
            let rest = if double { &p[2..] } else { &p[1..] };
            for i in 0..=t.len() {
                if !double && t[..i].contains(&b'/') { break; }
                if go(rest, &t[i..]) { return true; }
            }
            return false;
        }
        if t.is_empty() { return false; }
        if p[0] == b'?' || p[0] == t[0] { go(&p[1..], &t[1..]) } else { false }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

struct EditFile;
impl Tool for EditFile {
    fn name(&self) -> &'static str { "edit_file" }
    fn description(&self) -> &'static str { "Replace an exact, unique text snippet in a file. Safer than rewriting a whole file." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},
            "old_string":{"type":"string","description":"Exact text to replace (must be unique)"},
            "new_string":{"type":"string"}
        },"required":["path","old_string","new_string"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let old = arg_str(args, "old_string")?;
        let new = arg_str(args, "new_string")?;
        if old.is_empty() { return Err("old_string is empty".into()); }
        let text = fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))?;
        let count = text.match_indices(&old).count();
        if count == 0 { return Err("old_string not found in file".into()); }
        if count > 1 { return Err(format!("old_string appears {count} times; include more context to make it unique")); }
        fs::write(&path, text.replacen(&old, &new, 1)).map_err(|e| format!("write failed: {e}"))?;
        Ok(format!("edited {}", path.display()))
    }
}

pub(crate) fn git(ctx: &ToolContext, args: &[&str]) -> Result<String, String> {
    let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
    let out = Command::new("git").args(args).current_dir(&root).output().map_err(|e| format!("git failed: {e}"))?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.stderr.is_empty() {
        s.push_str("\n[stderr]\n");
        s.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    Ok(truncate(s, 12000))
}

struct GitStatus;
impl Tool for GitStatus {
    fn name(&self) -> &'static str { "git_status" }
    fn description(&self) -> &'static str { "Show the working tree status (short format)." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> {
        git(ctx, &["status", "--short", "--branch"])
    }
}

struct GitDiff;
impl Tool for GitDiff {
    fn name(&self) -> &'static str { "git_diff" }
    fn description(&self) -> &'static str { "Show uncommitted changes as a unified diff. Optionally pass a file path." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        match args.get("path").and_then(Value::as_str) {
            Some(p) => {
                let path = safe_path(ctx, p)?;
                let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
                let rel = path.strip_prefix(&root).map_err(|_| "path outside workspace")?.to_string_lossy();
                git(ctx, &["diff", "--", &rel])
            }
            None => git(ctx, &["diff"]),
        }
    }
}

struct GitLog;
impl Tool for GitLog {
    fn name(&self) -> &'static str { "git_log" }
    fn description(&self) -> &'static str { "Show the last commits (oneline)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"count":{"type":"integer","description":"default 10"}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let n = args.get("count").and_then(Value::as_u64).unwrap_or(10).min(50);
        git(ctx, &["log", "--oneline", "-n", &n.to_string()])
    }
}

struct WebFetch;
impl Tool for WebFetch {
    fn name(&self) -> &'static str { "web_fetch" }
    fn description(&self) -> &'static str { "Fetch an http(s) URL as plain text (max 64 KB). Read-only." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"],"additionalProperties":false})
    }
    fn execute(&self, _: &ToolContext, args: &Value) -> Result<String, String> {
        let url = arg_str(args, "url")?;
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("only http(s) URLs are allowed".into());
        }
        let response = ureq::get(&url).call().map_err(|e| format!("fetch failed: {e}"))?;
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 8192];
        while bytes.len() < 64 * 1024 {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            bytes.extend_from_slice(&buf[..n]);
        }
        Ok(truncate(String::from_utf8_lossy(&bytes).into_owned(), 16000))
    }
}

struct SkillLoad;
impl Tool for SkillLoad {
    fn name(&self) -> &'static str { "skill_load" }
    fn description(&self) -> &'static str { "Read the full instructions of a skill listed in your system prompt." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let name = arg_str(args, "name")?;
        match ctx.skills.iter().find(|s| s.name.eq_ignore_ascii_case(&name)) {
            Some(s) => Ok(format!("# {}\n\n{}\n\n[source: {}]", s.name, s.body, s.source)),
            None => Err(format!("unknown skill {name:?}; see the skill catalog in your system prompt")),
        }
    }
}

struct TodoWrite;
impl Tool for TodoWrite {
    fn name(&self) -> &'static str { "todo_write" }
    fn description(&self) -> &'static str { "Record a short plan for the current task (shown as a checklist in the transcript)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"items":{"type":"array","items":{"type":"string"}}},"required":["items"],"additionalProperties":false})
    }
    fn execute(&self, _: &ToolContext, args: &Value) -> Result<String, String> {
        let items = args.get("items").and_then(Value::as_array).ok_or("items must be an array")?;
        let mut out = String::from("Plan:\n");
        let mut state = crate::tools_extra::todo_state().lock().map_err(|e| e.to_string())?;
        state.clear();
        for (i, item) in items.iter().enumerate() {
            let text = item.as_str().unwrap_or("").to_string();
            out.push_str(&format!("  [ ] {}. {}\n", i + 1, text));
            state.push(text);
        }
        Ok(out)
    }
}

struct MemoryWrite;
impl Tool for MemoryWrite {
    fn name(&self) -> &'static str { "memory_write" }
    fn description(&self) -> &'static str { "Append a durable note to .rivo/memory.md in the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"note":{"type":"string"}},"required":["note"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let note = arg_str(args, "note")?;
        let dir = ctx.workspace.join(".rivo");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("memory.md");
        let mut prior = fs::read_to_string(&path).unwrap_or_default();
        if !prior.is_empty() { prior.push('\n'); }
        fs::write(&path, format!("{prior}- {note}\n")).map_err(|e| e.to_string())?;
        Ok("saved to .rivo/memory.md".into())
    }
}

struct MemoryRead;
impl Tool for MemoryRead {
    fn name(&self) -> &'static str { "memory_read" }
    fn description(&self) -> &'static str { "Read durable notes from .rivo/memory.md." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> {
        let path = ctx.workspace.join(".rivo/memory.md");
        match fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => Ok(truncate(text, 8000)),
            _ => Ok("(no saved notes)".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ToolContext {
        ToolContext::new(std::env::temp_dir(), PermissionMode::Auto, Vec::new())
    }

    #[test]
    fn glob_matcher_handles_star_and_double_star() {
        assert!(glob_match("*.toml", "Cargo.toml"));
        assert!(!glob_match("*.toml", "src/Cargo.toml"));
        assert!(glob_match("src/**/*.rs", "src/a/b.rs"));
        assert!(glob_match("src/**/*.rs", "src/main.rs"));
    }

    #[test]
    fn read_only_mode_blocks_writes() {
        let c = ToolContext::new(std::env::temp_dir(), PermissionMode::ReadOnly, Vec::new());
        assert!(WriteFile.execute(&c, &json!({"path": "x", "content": "y"})).is_err());
        assert!(RunCommand.execute(&c, &json!({"command": "pwd"})).is_err());
    }

    #[test]
    fn command_allowlist_blocks_destructive_input() {
        let c = ctx();
        assert!(RunCommand.execute(&c, &json!({"command": "rm -rf /"})).is_err());
        assert!(RunCommand.execute(&c, &json!({"command": "git status && rm x"})).is_err());
    }

    #[test]
    fn paths_may_not_escape() {
        let c = ctx();
        assert!(safe_path(&c, "/etc/passwd").is_err());
        assert!(safe_path(&c, "../../etc/passwd").is_err());
    }

    #[test]
    fn tool_catalog_has_fifty_unique_tools() {
        let tools = all_tools();
        assert_eq!(tools.len(), 50);
        let mut names: Vec<_> = tools.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        assert!(names.windows(2).all(|w| w[0] != w[1]), "duplicate tool names");
    }
}
