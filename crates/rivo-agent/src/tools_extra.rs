//! Additional built-in tools: file management, richer search, git, background tasks,
//! web helpers and session utilities. All filesystem paths go through `safe_path`, so the
//! same workspace confinement as the original tools applies here.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext, arg_str, collect_files, command_parts, git, glob_match, safe_path, truncate};

/// Session plan shared by `todo_write`, `todo_read` and `todo_clear` (across turns).
pub(crate) fn todo_state() -> &'static Mutex<Vec<String>> {
    static TODOS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    TODOS.get_or_init(|| Mutex::new(Vec::new()))
}

struct BgTask {
    command: String,
    log: std::path::PathBuf,
    child: Mutex<Child>,
    started: u64,
}

fn task_state() -> &'static Mutex<HashMap<String, Arc<BgTask>>> {
    static TASKS: OnceLock<Mutex<HashMap<String, Arc<BgTask>>>> = OnceLock::new();
    TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(CreateDir), Box::new(DeleteFile), Box::new(MoveFile), Box::new(CopyFile),
        Box::new(AppendFile), Box::new(FileInfo), Box::new(MultiEdit), Box::new(ApplyPatch),
        Box::new(ReadLines), Box::new(DiffFiles),
        Box::new(Grep), Box::new(FindSymbol), Box::new(RecentFiles), Box::new(CountLines),
        Box::new(GitBranch), Box::new(GitShow), Box::new(GitBlame), Box::new(GitAdd),
        Box::new(GitCommit), Box::new(GitRestore), Box::new(GitStash), Box::new(GitTags),
        Box::new(GitRemote), Box::new(GitGrep),
        Box::new(RunCommandBg), Box::new(TaskOutput), Box::new(TaskStop), Box::new(TaskList),
        Box::new(WebSearch), Box::new(HttpRequest), Box::new(DownloadFile),
        Box::new(TodoRead), Box::new(TodoClear), Box::new(ListSkills), Box::new(SystemInfo),
    ]
}

fn rel(ctx: &ToolContext, path: &Path) -> String {
    let root = ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone());
    path.strip_prefix(&root).unwrap_or(path).to_string_lossy().into_owned()
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn tail(s: String, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max { return s; }
    let start = chars.len() - max;
    format!("… [truncated]\n{}", chars[start..].iter().collect::<String>())
}

// ── Filesystem & editing ──────────────────────────────────────────────────────

struct CreateDir;
impl Tool for CreateDir {
    fn name(&self) -> &'static str { "create_dir" }
    fn description(&self) -> &'static str { "Create a directory (and missing parents) inside the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(format!("created directory {}", rel(ctx, &path)))
    }
}

struct DeleteFile;
impl Tool for DeleteFile {
    fn name(&self) -> &'static str { "delete_file" }
    fn description(&self) -> &'static str { "Delete a file or empty directory. Set recursive=true for a directory tree." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},"recursive":{"type":"boolean","description":"delete a directory with its contents"}
        },"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
        if path == root { return Err("refusing to delete the workspace root".into()); }
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            if args.get("recursive").and_then(Value::as_bool).unwrap_or(false) {
                fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            } else {
                fs::remove_dir(&path).map_err(|e| e.to_string())?;
            }
        } else {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        Ok(format!("deleted {}", rel(ctx, &path)))
    }
}

struct MoveFile;
impl Tool for MoveFile {
    fn name(&self) -> &'static str { "move_file" }
    fn description(&self) -> &'static str { "Move or rename a file or directory inside the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let from = safe_path(ctx, &arg_str(args, "from")?)?;
        let to = safe_path(ctx, &arg_str(args, "to")?)?;
        if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        match fs::rename(&from, &to) {
            Ok(()) => Ok(format!("moved {} → {}", rel(ctx, &from), rel(ctx, &to))),
            Err(_) if from.is_file() => {
                fs::copy(&from, &to).map_err(|e| e.to_string())?;
                fs::remove_file(&from).map_err(|e| e.to_string())?;
                Ok(format!("moved {} → {}", rel(ctx, &from), rel(ctx, &to)))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

struct CopyFile;
impl Tool for CopyFile {
    fn name(&self) -> &'static str { "copy_file" }
    fn description(&self) -> &'static str { "Copy a file, or a directory tree when recursive=true." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "from":{"type":"string"},"to":{"type":"string"},"recursive":{"type":"boolean"}
        },"required":["from","to"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let from = safe_path(ctx, &arg_str(args, "from")?)?;
        let to = safe_path(ctx, &arg_str(args, "to")?)?;
        if from.is_dir() {
            if !args.get("recursive").and_then(Value::as_bool).unwrap_or(false) {
                return Err("source is a directory; set recursive=true".into());
            }
            copy_tree(&from, &to, &from)?;
        } else {
            if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
        Ok(format!("copied {} → {}", rel(ctx, &from), rel(ctx, &to)))
    }
}

fn copy_tree(from: &Path, to: &Path, root: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() { copy_tree(&src, &dst, root)?; }
        else { fs::copy(&src, &dst).map_err(|e| e.to_string())?; }
    }
    Ok(())
}

struct AppendFile;
impl Tool for AppendFile {
    fn name(&self) -> &'static str { "append_file" }
    fn description(&self) -> &'static str { "Append text to a file, creating it if missing." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let content = arg_str(args, "content")?;
        let mut f = OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
        f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        Ok(format!("appended {} bytes to {}", content.len(), rel(ctx, &path)))
    }
}

struct FileInfo;
impl Tool for FileInfo {
    fn name(&self) -> &'static str { "file_info" }
    fn description(&self) -> &'static str { "Show a file or directory's kind, size and modification time." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let kind = if m.is_dir() { "directory" } else if m.file_type().is_symlink() { "symlink" } else { "file" };
        Ok(format!("{}: {kind}, {} bytes, modified at {}s since epoch", rel(ctx, &path), m.len(), mtime(&path)))
    }
}

struct MultiEdit;
impl Tool for MultiEdit {
    fn name(&self) -> &'static str { "multi_edit" }
    fn description(&self) -> &'static str { "Apply several exact, unique text replacements to one file in a single write." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},
            "edits":{"type":"array","items":{"type":"object","properties":{
                "old_string":{"type":"string"},"new_string":{"type":"string"}
            },"required":["old_string","new_string"]}}
        },"required":["path","edits"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let edits = args.get("edits").and_then(Value::as_array).ok_or("edits must be an array")?;
        let mut text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        for (i, edit) in edits.iter().enumerate() {
            let old = arg_str(edit, "old_string")?;
            let new = arg_str(edit, "new_string")?;
            if old.is_empty() { return Err(format!("edit {}: old_string is empty", i + 1)); }
            let count = text.match_indices(&old).count();
            if count == 0 { return Err(format!("edit {}: old_string not found", i + 1)); }
            if count > 1 { return Err(format!("edit {}: old_string appears {count} times", i + 1)); }
            text = text.replacen(&old, &new, 1);
        }
        fs::write(&path, &text).map_err(|e| e.to_string())?;
        Ok(format!("applied {} edits to {}", edits.len(), rel(ctx, &path)))
    }
}

struct ApplyPatch;
impl Tool for ApplyPatch {
    fn name(&self) -> &'static str { "apply_patch" }
    fn description(&self) -> &'static str {
        "Apply a multi-file patch (*** Begin Patch / Add File / Update File / Delete File / @@ hunks / *** End Patch)."
    }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"patch":{"type":"string","description":"Patch text"}},"required":["patch"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let patch = arg_str(args, "patch")?;
        let lines: Vec<&str> = patch.lines().collect();
        let mut i = 0usize;
        let mut changes: Vec<(std::path::PathBuf, Option<String>)> = Vec::new(); // None = delete
        while i < lines.len() {
            let line = lines[i].trim();
            if let Some(p) = line.strip_prefix("*** Add File:") {
                let path = safe_path(ctx, p.trim())?;
                i += 1;
                let mut body = String::new();
                while i < lines.len() && !lines[i].starts_with("*** ") {
                    if let Some(rest) = lines[i].strip_prefix('+') { body.push_str(rest); body.push('\n'); }
                    i += 1;
                }
                changes.push((path, Some(body)));
            } else if let Some(p) = line.strip_prefix("*** Delete File:") {
                changes.push((safe_path(ctx, p.trim())?, None));
                i += 1;
            } else if let Some(p) = line.strip_prefix("*** Update File:") {
                let path = safe_path(ctx, p.trim())?;
                i += 1;
                let mut section = Vec::new();
                while i < lines.len() && !lines[i].starts_with("*** ") {
                    section.push(lines[i].to_string());
                    i += 1;
                }
                let existing = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let hunks = parse_hunks(&section);
                changes.push((path, Some(apply_hunks(&existing, &hunks)?)));
            } else {
                i += 1;
            }
        }
        if changes.is_empty() { return Err("no file operations found in patch".into()); }
        for (path, content) in &changes {
            match content {
                Some(text) => {
                    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                    fs::write(path, text).map_err(|e| e.to_string())?;
                }
                None => { fs::remove_file(path).map_err(|e| e.to_string())?; }
            }
        }
        Ok(format!("applied patch to {} file(s)", changes.len()))
    }
}

struct Hunk { old: Vec<String>, new: Vec<String>, hint: usize }

fn parse_hunks(section: &[String]) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in section {
        if line.starts_with("@@") {
            if let Some(c) = current.take() { hunks.push(make_hunk(&c)); }
            current = Some(vec![line.clone()]);
        } else {
            let meaningful = line.starts_with(' ') || line.starts_with('-') || line.starts_with('+');
            if meaningful || line.is_empty() {
                if current.is_none() { current = Some(Vec::new()); }
                current.as_mut().unwrap().push(line.clone());
            }
        }
    }
    if let Some(c) = current { hunks.push(make_hunk(&c)); }
    hunks
}

fn make_hunk(lines: &[String]) -> Hunk {
    let mut old = Vec::new();
    let mut new = Vec::new();
    let mut hint = 0usize;
    for line in lines {
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some(rest) = rest.strip_prefix(" -") {
                let num: String = rest.chars().take_while(char::is_ascii_digit).collect();
                hint = num.parse().unwrap_or(0);
            }
        } else if let Some(rest) = line.strip_prefix(' ') {
            old.push(rest.to_string());
            new.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('-') {
            old.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('+') {
            new.push(rest.to_string());
        } else if line.is_empty() {
            old.push(String::new());
            new.push(String::new());
        }
    }
    Hunk { old, new, hint }
}

fn apply_hunks(text: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let trailing_nl = text.ends_with('\n');
    for h in hunks {
        let mut at = h.hint.saturating_sub(1).min(lines.len());
        if h.old.is_empty() {
            for (k, line) in h.new.iter().enumerate() { lines.insert(at + k, line.clone()); }
            continue;
        }
        if lines.len() < h.old.len() { return Err("patch does not match: file too short".into()); }
        let end = lines.len() - h.old.len();
        at = at.min(end);
        let mut found = None;
        for i in at..=end {
            if lines[i..i + h.old.len()] == h.old[..] { found = Some(i); break; }
        }
        if found.is_none() && at > 0 {
            for i in 0..at {
                if lines[i..i + h.old.len()] == h.old[..] { found = Some(i); break; }
            }
        }
        let idx = found.ok_or_else(|| "patch does not match: context not found".to_string())?;
        lines.splice(idx..idx + h.old.len(), h.new.iter().cloned());
    }
    let mut out = lines.join("\n");
    if trailing_nl { out.push('\n'); }
    Ok(out)
}

struct ReadLines;
impl Tool for ReadLines {
    fn name(&self) -> &'static str { "read_lines" }
    fn description(&self) -> &'static str { "Read a numbered line range of a file (offset is 1-based)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},"offset":{"type":"integer","description":"first line, default 1"},
            "limit":{"type":"integer","description":"lines to read, default 80"}
        },"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(80).clamp(1, 2000) as usize;
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut out = String::new();
        for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
            out.push_str(&format!("{:>5}| {}\n", i + 1, line));
        }
        Ok(if out.is_empty() { format!("(no lines from {offset})") } else { out })
    }
}

struct DiffFiles;
impl Tool for DiffFiles {
    fn name(&self) -> &'static str { "diff_files" }
    fn description(&self) -> &'static str { "Show a unified diff between two text files in the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path_a":{"type":"string"},"path_b":{"type":"string"}},"required":["path_a","path_b"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let a = safe_path(ctx, &arg_str(args, "path_a")?)?;
        let b = safe_path(ctx, &arg_str(args, "path_b")?)?;
        let ta = fs::read_to_string(&a).map_err(|e| e.to_string())?;
        let tb = fs::read_to_string(&b).map_err(|e| e.to_string())?;
        let al: Vec<String> = ta.lines().map(str::to_owned).collect();
        let bl: Vec<String> = tb.lines().map(str::to_owned).collect();
        Ok(truncate(unified_diff(&al, &bl, &rel(ctx, &a), &rel(ctx, &b)), 16000))
    }
}

fn unified_diff(a: &[String], b: &[String], name_a: &str, name_b: &str) -> String {
    let ops = if a.len() * b.len() <= 100_000 { lcs_diff(a, b) } else { naive_diff(a, b) };
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (c, _, _))| *c != ' ').map(|(i, _)| i).collect();
    if changed.is_empty() { return "(files are identical)".into(); }
    let mut out = format!("--- {name_a}\n+++ {name_b}\n");
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &idx in &changed {
        if let Some(last) = groups.last_mut() {
            if idx <= last.1 + 4 { last.1 = idx; continue; }
        }
        groups.push((idx, idx));
    }
    for (first, last) in groups {
        let start = first.saturating_sub(2);
        let end = (last + 3).min(ops.len());
        let old_before = ops[..start].iter().filter(|(c, _, _)| *c != '+').count();
        let new_before = ops[..start].iter().filter(|(c, _, _)| *c != '-').count();
        let old_count = ops[start..end].iter().filter(|(c, _, _)| *c != '+').count();
        let new_count = ops[start..end].iter().filter(|(c, _, _)| *c != '-').count();
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", old_before + 1, old_count, new_before + 1, new_count));
        for (c, _, text) in &ops[start..end] { out.push_str(&format!("{c}{text}\n")); }
    }
    out
}

fn lcs_diff(a: &[String], b: &[String]) -> Vec<(char, usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0usize, 0usize);
    let mut ops = Vec::new();
    while i < n && j < m {
        if a[i] == b[j] { ops.push((' ', i, j)); i += 1; j += 1; }
        else if dp[i + 1][j] >= dp[i][j + 1] { ops.push(('-', i, j)); i += 1; }
        else { ops.push(('+', i, j)); j += 1; }
    }
    while i < n { ops.push(('-', i, j)); i += 1; }
    while j < m { ops.push(('+', i, j)); j += 1; }
    ops
}

fn naive_diff(a: &[String], b: &[String]) -> Vec<(char, usize, usize)> {
    let mut ops = Vec::new();
    let shared = a.len().min(b.len());
    for i in 0..shared {
        if a[i] == b[i] { ops.push((' ', i, i)); } else { ops.push(('-', i, i)); ops.push(('+', i, i)); }
    }
    for i in shared..a.len() { ops.push(('-', i, shared)); }
    for j in shared..b.len() { ops.push(('+', shared, j)); }
    ops
}

// ── Search & discovery ────────────────────────────────────────────────────────

struct Grep;
impl Tool for Grep {
    fn name(&self) -> &'static str { "grep" }
    fn description(&self) -> &'static str {
        "Search line contents with a wildcard pattern (`*`, `?`) or literal text; optionally filter files by glob."
    }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "pattern":{"type":"string"},"path":{"type":"string"},"include":{"type":"string"},
            "case_sensitive":{"type":"boolean"},"limit":{"type":"integer"}
        },"required":["pattern"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let pattern = arg_str(args, "pattern")?;
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let start = safe_path(ctx, raw)?;
        let include = args.get("include").and_then(Value::as_str);
        let cs = args.get("case_sensitive").and_then(Value::as_bool).unwrap_or(true);
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 500) as usize;
        let root = ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone());
        let mut files = Vec::new();
        collect_files(&start, &mut files, 0);
        let mut out = String::new();
        let mut hits = 0usize;
        for f in files {
            let r = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            if include.is_some_and(|inc| !glob_match(inc, &r)) { continue; }
            let Ok(bytes) = fs::read(&f) else { continue };
            if bytes.contains(&0) || bytes.len() > 2_000_000 { continue; }
            let text = String::from_utf8_lossy(&bytes);
            for (i, line) in text.lines().enumerate() {
                let lower;
                let (p, l) = if cs {
                    (pattern.as_str(), line)
                } else {
                    lower = (pattern.to_lowercase(), line.to_lowercase());
                    (lower.0.as_str(), lower.1.as_str())
                };
                if glob_match(p, l) {
                    out.push_str(&format!("{r}:{}: {}\n", i + 1, line.trim()));
                    hits += 1;
                    if hits >= limit { return Ok(truncate(out, 20000)); }
                }
            }
        }
        Ok(if out.is_empty() { format!("no matches for {pattern:?}") } else { out })
    }
}

struct FindSymbol;
impl Tool for FindSymbol {
    fn name(&self) -> &'static str { "find_symbol" }
    fn description(&self) -> &'static str { "Find declarations of a symbol (fn, struct, class, const, type, ...) in the workspace." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"name":{"type":"string"},"path":{"type":"string"}},"required":["name"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let name = arg_str(args, "name")?;
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let start = safe_path(ctx, raw)?;
        let root = ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone());
        const DECL: &[&str] = &["fn ", "struct ", "enum ", "trait ", "impl ", "const ", "static ",
            "class ", "def ", "interface ", "type ", "record ", "pub ", "async "];
        let mut files = Vec::new();
        collect_files(&start, &mut files, 0);
        let mut out = String::new();
        for f in files {
            let Ok(bytes) = fs::read(&f) else { continue };
            if bytes.contains(&0) || bytes.len() > 2_000_000 { continue; }
            let text = String::from_utf8_lossy(&bytes);
            for (i, line) in text.lines().enumerate() {
                if line.contains(&name) && DECL.iter().any(|d| line.contains(d)) {
                    let r = f.strip_prefix(&root).unwrap_or(&f).display();
                    out.push_str(&format!("{r}:{}: {}\n", i + 1, line.trim()));
                }
            }
        }
        Ok(if out.is_empty() { format!("no declaration found for {name:?}") } else { truncate(out, 12000) })
    }
}

struct RecentFiles;
impl Tool for RecentFiles {
    fn name(&self) -> &'static str { "recent_files" }
    fn description(&self) -> &'static str { "List the most recently modified workspace files." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let start = safe_path(ctx, raw)?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(25).clamp(1, 100) as usize;
        let root = ctx.workspace.canonicalize().unwrap_or(ctx.workspace.clone());
        let mut files = Vec::new();
        collect_files(&start, &mut files, 0);
        files.sort_by_key(|f| std::cmp::Reverse(mtime(f)));
        let mut out = String::new();
        for f in files.into_iter().take(limit) {
            let r = f.strip_prefix(&root).unwrap_or(&f).display();
            out.push_str(&format!("{}  {r}\n", mtime(&f)));
        }
        Ok(if out.is_empty() { "(no files)".into() } else { out })
    }
}

struct CountLines;
impl Tool for CountLines {
    fn name(&self) -> &'static str { "count_lines" }
    fn description(&self) -> &'static str { "Count lines, words, characters and bytes in a file." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(format!("{}: {} lines, {} words, {} chars, {} bytes",
            rel(ctx, &path), text.lines().count(), text.split_whitespace().count(), text.chars().count(), bytes.len()))
    }
}

// ── Git ───────────────────────────────────────────────────────────────────────

fn git_rel(ctx: &ToolContext, raw: &str) -> Result<String, String> {
    let path = safe_path(ctx, raw)?;
    let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
    Ok(path.strip_prefix(&root).map_err(|_| "path outside workspace")?.to_string_lossy().into_owned())
}

struct GitBranch;
impl Tool for GitBranch {
    fn name(&self) -> &'static str { "git_branch" }
    fn description(&self) -> &'static str { "List local branches, or all branches with all=true." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"all":{"type":"boolean"}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        if args.get("all").and_then(Value::as_bool).unwrap_or(false) { git(ctx, &["branch", "-a"]) }
        else { git(ctx, &["branch"]) }
    }
}

struct GitShow;
impl Tool for GitShow {
    fn name(&self) -> &'static str { "git_show" }
    fn description(&self) -> &'static str { "Show a commit's summary, or its full patch with patch=true." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"target":{"type":"string"},"patch":{"type":"boolean"}},"required":["target"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let target = arg_str(args, "target")?;
        if args.get("patch").and_then(Value::as_bool).unwrap_or(false) { git(ctx, &["show", &target]) }
        else { git(ctx, &["show", "--stat", "--oneline", &target]) }
    }
}

struct GitBlame;
impl Tool for GitBlame {
    fn name(&self) -> &'static str { "git_blame" }
    fn description(&self) -> &'static str { "Show line-by-line authorship for a tracked file." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let r = git_rel(ctx, &arg_str(args, "path")?)?;
        git(ctx, &["blame", "--", &r])
    }
}

struct GitAdd;
impl Tool for GitAdd {
    fn name(&self) -> &'static str { "git_add" }
    fn description(&self) -> &'static str { "Stage files for commit (paths default to '.')." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}}},"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let paths: Vec<String> = args.get("paths").and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_else(|| vec![".".into()]);
        let mut rels = Vec::new();
        for p in &paths { rels.push(git_rel(ctx, p)?); }
        let mut av = vec!["add", "--"];
        for r in &rels { av.push(r); }
        git(ctx, &av)
    }
}

struct GitCommit;
impl Tool for GitCommit {
    fn name(&self) -> &'static str { "git_commit" }
    fn description(&self) -> &'static str { "Commit staged changes with a message." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let message = arg_str(args, "message")?;
        git(ctx, &["commit", "-m", &message])
    }
}

struct GitRestore;
impl Tool for GitRestore {
    fn name(&self) -> &'static str { "git_restore" }
    fn description(&self) -> &'static str { "Restore a file from the index (or HEAD with staged=true)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"staged":{"type":"boolean"}},"required":["path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let r = git_rel(ctx, &arg_str(args, "path")?)?;
        if args.get("staged").and_then(Value::as_bool).unwrap_or(false) { git(ctx, &["restore", "--staged", "--", &r]) }
        else { git(ctx, &["restore", "--", &r]) }
    }
}

struct GitStash;
impl Tool for GitStash {
    fn name(&self) -> &'static str { "git_stash" }
    fn description(&self) -> &'static str { "Manage stashes: action = list, push, pop or drop." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"action":{"type":"string","enum":["list","push","pop","drop"]},"message":{"type":"string"}},"required":["action"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let action = arg_str(args, "action")?;
        let message = args.get("message").and_then(Value::as_str).unwrap_or("").to_string();
        match action.as_str() {
            "list" => git(ctx, &["stash", "list"]),
            "push" => {
                ctx.writable()?;
                if message.is_empty() { git(ctx, &["stash", "push"]) } else { git(ctx, &["stash", "push", "-m", &message]) }
            }
            "pop" => { ctx.writable()?; git(ctx, &["stash", "pop"]) }
            "drop" => { ctx.writable()?; git(ctx, &["stash", "drop"]) }
            _ => Err("action must be list, push, pop or drop".into()),
        }
    }
}

struct GitTags;
impl Tool for GitTags {
    fn name(&self) -> &'static str { "git_tags" }
    fn description(&self) -> &'static str { "List tags with their messages." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> { git(ctx, &["tag", "-n"]) }
}

struct GitRemote;
impl Tool for GitRemote {
    fn name(&self) -> &'static str { "git_remote" }
    fn description(&self) -> &'static str { "List configured remotes with their URLs." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> { git(ctx, &["remote", "-v"]) }
}

struct GitGrep;
impl Tool for GitGrep {
    fn name(&self) -> &'static str { "git_grep" }
    fn description(&self) -> &'static str { "Search tracked files with git grep (regex, file:line output)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let pattern = arg_str(args, "pattern")?;
        match args.get("path").and_then(Value::as_str) {
            Some(p) => { let r = git_rel(ctx, p)?; git(ctx, &["grep", "-n", "-e", &pattern, "--", &r]) }
            None => git(ctx, &["grep", "-n", "-e", &pattern]),
        }
    }
}

// ── Background tasks ──────────────────────────────────────────────────────────

struct RunCommandBg;
impl Tool for RunCommandBg {
    fn name(&self) -> &'static str { "run_command_bg" }
    fn description(&self) -> &'static str { "Start an allowlisted command in the background; output is logged for task_output." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let command = arg_str(args, "command")?;
        let parts = command_parts(&command)?;
        let root = ctx.workspace.canonicalize().map_err(|e| e.to_string())?;
        let dir = root.join(".rivo/tasks");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let id = format!("task-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis());
        let log = dir.join(format!("{id}.log"));
        let out = File::create(&log).map_err(|e| e.to_string())?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let child = Command::new(&parts[0]).args(&parts[1..]).current_dir(&root)
            .stdout(Stdio::from(out)).stderr(Stdio::from(err)).spawn().map_err(|e| e.to_string())?;
        task_state().lock().map_err(|e| e.to_string())?.insert(id.clone(), Arc::new(BgTask {
            command, log: log.clone(), child: Mutex::new(child),
            started: now_secs(),
        }));
        Ok(format!("started {id}: {}\nlog: {}", parts.join(" "), rel(ctx, &log)))
    }
}

struct TaskOutput;
impl Tool for TaskOutput {
    fn name(&self) -> &'static str { "task_output" }
    fn description(&self) -> &'static str { "Read the latest output of a background task by id." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"id":{"type":"string"},"limit":{"type":"integer"}},"required":["id"],"additionalProperties":false})
    }
    fn execute(&self, _: &ToolContext, args: &Value) -> Result<String, String> {
        let id = arg_str(args, "id")?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(8000) as usize;
        let tasks = task_state().lock().map_err(|e| e.to_string())?;
        let task = tasks.get(&id).ok_or_else(|| format!("unknown task {id:?}"))?;
        let text = fs::read_to_string(&task.log).map_err(|e| e.to_string())?;
        Ok(if text.trim().is_empty() { "(no output yet)".into() } else { tail(text, limit) })
    }
}

struct TaskStop;
impl Tool for TaskStop {
    fn name(&self) -> &'static str { "task_stop" }
    fn description(&self) -> &'static str { "Stop a running background task by id." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false})
    }
    fn execute(&self, _: &ToolContext, args: &Value) -> Result<String, String> {
        let id = arg_str(args, "id")?;
        let tasks = task_state().lock().map_err(|e| e.to_string())?;
        let task = tasks.get(&id).ok_or_else(|| format!("unknown task {id:?}"))?;
        let mut child = task.child.lock().map_err(|e| e.to_string())?;
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => Ok(format!("{id} already exited with {status}")),
            None => { child.kill().map_err(|e| e.to_string())?; let _ = child.wait(); Ok(format!("stopped {id}")) }
        }
    }
}

struct TaskList;
impl Tool for TaskList {
    fn name(&self) -> &'static str { "task_list" }
    fn description(&self) -> &'static str { "List background tasks with their status." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, _: &ToolContext, _: &Value) -> Result<String, String> {
        let tasks = task_state().lock().map_err(|e| e.to_string())?;
        if tasks.is_empty() { return Ok("(no background tasks)".into()); }
        let mut out = String::new();
        for (id, task) in tasks.iter() {
            let status = match task.child.lock().map_err(|e| e.to_string())?.try_wait() {
                Ok(Some(s)) => format!("exited ({s})"),
                Ok(None) => "running".into(),
                Err(e) => format!("error: {e}"),
            };
            out.push_str(&format!("{id}  [{status}]  {}  (started {}s)\n", task.command, task.started));
        }
        Ok(out)
    }
}

// ── Web ───────────────────────────────────────────────────────────────────────

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { out.push(b as char); }
        else { out.push_str(&format!("%{b:02X}")); }
    }
    out
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#x27;", "'")
        .replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let a = s.find(start)? + start.len();
    let b = s[a..].find(end)? + a;
    Some(&s[a..b])
}

struct WebSearch;
impl Tool for WebSearch {
    fn name(&self) -> &'static str { "web_search" }
    fn description(&self) -> &'static str { "Search the web via DuckDuckGo's public HTML endpoint (no API key)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query"],"additionalProperties":false})
    }
    fn execute(&self, _: &ToolContext, args: &Value) -> Result<String, String> {
        let query = arg_str(args, "query")?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(8).clamp(1, 20) as usize;
        let url = format!("https://html.duckduckgo.com/html/?q={}", url_encode(&query));
        let response = ureq::get(&url).set("User-Agent", "Mozilla/5.0 (Rivo coding agent)")
            .timeout(Duration::from_secs(20)).call().map_err(|e| format!("search failed: {e}"))?;
        let html = response.into_string().unwrap_or_default();
        let mut out = String::new();
        for (i, chunk) in html.split("result__a\"").skip(1).take(limit).enumerate() {
            let title = between(chunk, ">", "</a>").map(html_unescape).unwrap_or_default();
            let link = between(chunk, "href=\"", "\"").map(html_unescape).unwrap_or_default();
            let snippet = between(chunk, "result__snippet\">", "</a>").map(html_unescape).unwrap_or_default();
            if title.is_empty() && link.is_empty() { continue; }
            out.push_str(&format!("{}. {}\n   {}\n", i + 1, title.trim(), link.trim()));
            if !snippet.trim().is_empty() { out.push_str(&format!("   {}\n", snippet.trim())); }
        }
        Ok(if out.is_empty() { format!("no results for {query:?}") } else { truncate(out, 12000) })
    }
}

struct HttpRequest;
impl Tool for HttpRequest {
    fn name(&self) -> &'static str { "http_request" }
    fn description(&self) -> &'static str { "Send an HTTP request with optional headers and body. Mutating methods need Auto mode." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{
            "url":{"type":"string"},"method":{"type":"string","enum":["GET","POST","PUT","PATCH","DELETE","HEAD","OPTIONS"]},
            "headers":{"type":"object","additionalProperties":{"type":"string"}},
            "body":{"type":"string"}
        },"required":["url"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        let url = arg_str(args, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only http(s) URLs are allowed".into());
        }
        let method = args.get("method").and_then(Value::as_str).unwrap_or("GET").to_uppercase();
        if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"].contains(&method.as_str()) {
            return Err("unsupported HTTP method".into());
        }
        if method != "GET" && method != "HEAD" && method != "OPTIONS" { ctx.writable()?; }
        let mut req = ureq::request(&method, &url).timeout(Duration::from_secs(30));
        if let Some(headers) = args.get("headers").and_then(Value::as_object) {
            for (k, v) in headers {
                if let Some(v) = v.as_str() { req = req.set(k, v); }
            }
        }
        let body = args.get("body").and_then(Value::as_str).unwrap_or("");
        let response = if body.is_empty() { req.call() } else { req.send_string(body) }
            .map_err(|e| format!("request failed: {e}"))?;
        let status = response.status();
        let text = response.into_string().unwrap_or_default();
        Ok(format!("HTTP {status}\n{}", truncate(text, 16000)))
    }
}

struct DownloadFile;
impl Tool for DownloadFile {
    fn name(&self) -> &'static str { "download_file" }
    fn description(&self) -> &'static str { "Download an http(s) URL to a workspace file (max 8 MB)." }
    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"url":{"type":"string"},"path":{"type":"string"}},"required":["url","path"],"additionalProperties":false})
    }
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<String, String> {
        ctx.writable()?;
        let url = arg_str(args, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only http(s) URLs are allowed".into());
        }
        let path = safe_path(ctx, &arg_str(args, "path")?)?;
        let response = ureq::get(&url).timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?;
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 8192];
        while bytes.len() < 8 * 1024 * 1024 {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            bytes.extend_from_slice(&buf[..n]);
        }
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        Ok(format!("downloaded {} bytes to {}", bytes.len(), rel(ctx, &path)))
    }
}

// ── Session utilities ─────────────────────────────────────────────────────────

struct TodoRead;
impl Tool for TodoRead {
    fn name(&self) -> &'static str { "todo_read" }
    fn description(&self) -> &'static str { "Read the current session plan recorded by todo_write." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, _: &ToolContext, _: &Value) -> Result<String, String> {
        let items = todo_state().lock().map_err(|e| e.to_string())?;
        if items.is_empty() { return Ok("(no plan recorded)".into()); }
        let mut out = String::from("Plan:\n");
        for (i, item) in items.iter().enumerate() { out.push_str(&format!("  [ ] {}. {}\n", i + 1, item)); }
        Ok(out)
    }
}

struct TodoClear;
impl Tool for TodoClear {
    fn name(&self) -> &'static str { "todo_clear" }
    fn description(&self) -> &'static str { "Clear the session plan." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, _: &ToolContext, _: &Value) -> Result<String, String> {
        todo_state().lock().map_err(|e| e.to_string())?.clear();
        Ok("plan cleared".into())
    }
}

struct ListSkills;
impl Tool for ListSkills {
    fn name(&self) -> &'static str { "list_skills" }
    fn description(&self) -> &'static str { "List available skills with their names and descriptions." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> {
        if ctx.skills.is_empty() { return Ok("(no skills available)".into()); }
        let mut out = String::new();
        for s in &ctx.skills { out.push_str(&format!("- {} — {} [{}]\n", s.name, s.description, s.source)); }
        Ok(truncate(out, 8000))
    }
}

struct SystemInfo;
impl Tool for SystemInfo {
    fn name(&self) -> &'static str { "system_info" }
    fn description(&self) -> &'static str { "Report OS, architecture, workspace path and tool/skill counts." }
    fn schema(&self) -> Value { json!({"type":"object","properties":{},"additionalProperties":false}) }
    fn execute(&self, ctx: &ToolContext, _: &Value) -> Result<String, String> {
        Ok(format!("OS: {}\nArch: {}\nWorkspace: {}\nTools: {}\nSkills: {}\nPermission: {:?}",
            std::env::consts::OS, std::env::consts::ARCH, ctx.workspace.display(),
            crate::tools::all_tools().len(), ctx.skills.len(), ctx.permission))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PermissionMode;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn workspace() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("rivo-extra-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext::new(dir.to_path_buf(), PermissionMode::Auto, Vec::new())
    }

    #[test]
    fn multi_edit_applies_every_edit() {
        let dir = workspace();
        fs::write(dir.join("a.txt"), "alpha beta gamma\n").unwrap();
        let r = MultiEdit.execute(&ctx(&dir), &json!({"path":"a.txt","edits":[
            {"old_string":"beta","new_string":"BETA"},
            {"old_string":"gamma","new_string":"GAMMA"}
        ]})).unwrap();
        assert!(r.contains("applied 2 edits"));
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "alpha BETA GAMMA\n");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn apply_patch_adds_updates_and_deletes() {
        let dir = workspace();
        let patch = "*** Begin Patch\n*** Add File: new.txt\n+hello\n*** Update File: old.txt\n@@\n-old\n+new\n*** Delete File: gone.txt\n*** End Patch";
        fs::write(dir.join("old.txt"), "old\n").unwrap();
        fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        ApplyPatch.execute(&ctx(&dir), &json!({"patch":patch})).unwrap();
        assert_eq!(fs::read_to_string(dir.join("new.txt")).unwrap(), "hello\n");
        assert_eq!(fs::read_to_string(dir.join("old.txt")).unwrap(), "new\n");
        assert!(!dir.join("gone.txt").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn grep_filters_by_include_and_case() {
        let dir = workspace();
        fs::write(dir.join("a.rs"), "fn alpha() {}\n").unwrap();
        fs::write(dir.join("b.txt"), "fn ALPHA() {}\n").unwrap();
        let out = Grep.execute(&ctx(&dir), &json!({"pattern":"fn *alpha*","include":"*.rs"})).unwrap();
        assert!(out.contains("a.rs:1"));
        assert!(!out.contains("b.txt"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn write_tools_are_blocked_in_read_only_mode() {
        let dir = workspace();
        let c = ToolContext::new(dir.clone(), PermissionMode::ReadOnly, Vec::new());
        assert!(CreateDir.execute(&c, &json!({"path":"x"})).is_err());
        assert!(DeleteFile.execute(&c, &json!({"path":"x"})).is_err());
        assert!(GitCommit.execute(&c, &json!({"message":"m"})).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn new_paths_stay_confined() {
        let dir = workspace();
        let c = ctx(&dir);
        assert!(DeleteFile.execute(&c, &json!({"path":"../escape"})).is_err());
        assert!(MoveFile.execute(&c, &json!({"from":"a","to":"/tmp/escape"})).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn todo_round_trip() {
        let dir = workspace();
        let c = ctx(&dir);
        *todo_state().lock().unwrap() = vec!["first".into(), "second".into()];
        let out = TodoRead.execute(&c, &json!({})).unwrap();
        assert!(out.contains("first"));
        TodoClear.execute(&c, &json!({})).unwrap();
        assert_eq!(TodoRead.execute(&c, &json!({})).unwrap(), "(no plan recorded)");
        fs::remove_dir_all(dir).ok();
    }
}
