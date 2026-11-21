//! Git-backed checkpoints for Rivo: named snapshots of the workspace that the agent and the
//! inspector can create, list and restore.
//!
//! A checkpoint is a regular commit whose message starts with `rivo checkpoint:`. Restoring
//! tags the current HEAD first (`rivo-safety-<timestamp>`), so a restore can always be undone
//! even after `git reset --hard`.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const PREFIX: &str = "rivo checkpoint:";

/// One checkpoint listed in the inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub id: String,
    pub label: String,
    pub when: String,
}

fn git(workspace: &Path, args: &[&str]) -> Result<String, String> {
    let root = workspace.canonicalize().map_err(|e| e.to_string())?;
    let out = Command::new("git")
        .args(args)
        .current_dir(&root)
        .output()
        .map_err(|e| format!("git failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { format!("git {} failed", args.first().copied().unwrap_or("")) } else { stderr });
    }
    Ok(stdout)
}

fn dirty(workspace: &Path) -> Result<bool, String> {
    Ok(!git(workspace, &["status", "--porcelain"])?.is_empty())
}

fn short_head(workspace: &Path) -> Result<String, String> {
    git(workspace, &["rev-parse", "--short", "HEAD"])
}

/// Commit the current working tree as a checkpoint.
pub fn create(workspace: &Path, label: &str) -> Result<Checkpoint, String> {
    if !dirty(workspace)? {
        return Err("nothing to checkpoint — the working tree is clean".into());
    }
    let label = label.trim();
    let label = if label.is_empty() { "manual checkpoint" } else { label };
    git(workspace, &["add", "-A"])?;
    git(workspace, &["commit", "-m", &format!("{PREFIX} {label}")])?;
    let id = short_head(workspace)?;
    let when = git(workspace, &["log", "-1", "--format=%cI"])?;
    Ok(Checkpoint { id, label: label.to_string(), when })
}

/// List checkpoints, newest first.
pub fn list(workspace: &Path) -> Result<Vec<Checkpoint>, String> {
    let format = format!("%h%x1f%s%x1f%cI");
    let raw = git(workspace, &[
        "log", "--max-count=50", &format!("--format={format}"), &format!("--grep=^{PREFIX}"),
    ])?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut parts = line.splitn(3, '\x1f');
        let (Some(id), Some(subject), Some(when)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let label = subject.strip_prefix(PREFIX).unwrap_or(subject).trim().to_string();
        out.push(Checkpoint { id: id.to_string(), label, when: when.to_string() });
    }
    Ok(out)
}

fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Restore the workspace to a checkpoint. The previous HEAD is tagged first, so the caller
/// can offer an "Undo restore" button pointing at that tag.
pub fn restore(workspace: &Path, id: &str) -> Result<String, String> {
    if id.is_empty() || id.contains(' ') || id.starts_with('-') {
        return Err("invalid checkpoint id".into());
    }
    // The id may be a safety tag; both forms are accepted by rev-parse.
    git(workspace, &["rev-parse", "--verify", &format!("{id}^{{commit}}")])?;
    let safety = format!("rivo-safety-{}", timestamp());
    git(workspace, &["tag", "-f", &safety, "HEAD"])?;
    git(workspace, &["reset", "--hard", id])?;
    Ok(format!("Restored {id}. Undo with the rivo-safety tag {safety}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rivo-checkpoint-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]).unwrap();
        git(&dir, &["config", "user.email", "rivo@example.com"]).unwrap();
        git(&dir, &["config", "user.name", "Rivo"]).unwrap();
        dir
    }

    #[test]
    fn creates_lists_and_restores_checkpoints() {
        let dir = repo();
        std::fs::write(dir.join("a.txt"), "first\n").unwrap();
        let first = create(&dir, "before edit").unwrap();
        std::fs::write(dir.join("a.txt"), "second\n").unwrap();
        let second = create(&dir, "after edit").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "second\n");

        let items = list(&dir).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, second.id);
        assert!(items.iter().any(|c| c.id == first.id && c.label == "before edit"));

        restore(&dir, &first.id).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "first\n");
        // The pre-restore state is still reachable through the safety tag.
        let tags = git(&dir, &["tag", "--list", "rivo-safety-*"]).unwrap();
        assert!(!tags.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn refuses_to_checkpoint_a_clean_tree() {
        let dir = repo();
        let err = create(&dir, "empty").unwrap_err();
        assert!(err.contains("nothing to checkpoint"));
        std::fs::remove_dir_all(dir).ok();
    }
}
