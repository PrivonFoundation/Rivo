//! Agent skills: small `SKILL.md` playbooks with YAML-style frontmatter, discovered from the
//! workspace, the user config directory and a set of built-ins. The model sees a catalog in its
//! system prompt and can load a skill's full body with the `skill_load` tool.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub source: String,
}

/// A built-in skill, written as a `SKILL.md` body for consistency with user skills.
fn builtin(name: &str, description: &str, body: &str) -> Skill {
    Skill { name: name.into(), description: description.into(), body: body.into(), source: "built-in".into() }
}

pub fn builtin_skills() -> Vec<Skill> {
    vec![
        builtin(
            "code-review",
            "Review changed code for bugs, edge cases and security issues.",
            "# Code review\n\n1. Read the changed files and the surrounding code.\n2. Check correctness, error handling, boundary conditions and security.\n3. Report findings ordered by severity, each with `file:line` and a concrete fix.\n4. If nothing is wrong, say so briefly — don't invent issues.",
        ),
        builtin(
            "rust-expert",
            "Rust idioms: ownership, errors, generics, async and safe concurrency.",
            "# Rust expert\n\n- Prefer `Result` over panics in library paths; use `?` and `thiserror`-style enums.\n- Borrow instead of cloning; reach for `Arc` only when shared ownership is required.\n- Keep `unsafe` isolated and document the invariant.\n- Use iterators and `?` over manual loops when clearer; avoid premature abstraction.\n- Run `cargo test` and `cargo clippy` after changes.",
        ),
        builtin(
            "test-writer",
            "Write focused unit and integration tests for code you just changed.",
            "# Test writer\n\n1. Find the existing test style in the crate.\n2. Cover the happy path, one boundary and one failure case per behavior.\n3. Name tests after the behavior, not the implementation.\n4. Keep fixtures local and deterministic; no network in unit tests.\n5. Run the tests and report the result.",
        ),
        builtin(
            "context-drop",
            "Investigate a bug by forming hypotheses and gathering evidence before editing.",
            "# Context drop\n\n1. Restate the symptom precisely.\n2. List at least two hypotheses.\n3. Gather evidence with read-only tools (search, git diff, logs) before changing code.\n4. Report the confirmed cause and the smallest fix.\n5. Never delete user work to make a test pass.",
        ),
    ]
}

/// Parse a SKILL.md: `---` frontmatter with `name:` / `description:`, then the Markdown body.
fn parse(path: &Path, text: &str) -> Option<Skill> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = String::new();
    let mut description = String::new();
    for line in lines.by_ref() {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix("name:") {
            name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("description:") {
            description = v.trim().to_string();
        }
    }
    if name.is_empty() {
        return None;
    }
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if description.is_empty() {
        description = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches('#').trim().to_string();
    }
    Some(Skill {
        name,
        description,
        body,
        source: path.to_string_lossy().into_owned(),
    })
}

/// Agent Skills ship as a folder containing `SKILL.md`. Accept both that layout and a
/// flat `name.md` file, one level deep at most.
fn from_dir(dir: &Path, out: &mut Vec<Skill>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                add_skill(&skill_file, out);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let is_skill = path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md")
            || path.extension().and_then(|e| e.to_str()) == Some("md");
        if is_skill {
            add_skill(&path, out);
        }
    }
}

fn add_skill(path: &Path, out: &mut Vec<Skill>) {
    if let Ok(text) = fs::read_to_string(path) {
        if let Some(skill) = parse(path, &text) {
            if !out.iter().any(|s| s.name == skill.name) {
                out.push(skill);
            }
        }
    }
}

/// Built-ins first, then user skills (`~/.config/rivo/skills`), then workspace skills
/// (`<workspace>/.rivo/skills`) which win on name collisions.
pub fn discover(workspace: &Path) -> Vec<Skill> {
    let mut out = builtin_skills();
    if let Some(home) = std::env::var_os("HOME") {
        from_dir(&PathBuf::from(home).join(".config/rivo/skills"), &mut out);
    }
    from_dir(&workspace.join(".rivo/skills"), &mut out);
    out
}

pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let text = "---\nname: demo\ndescription: A demo skill\n---\n# Body\nDo the thing.\n";
        let skill = parse(Path::new("/tmp/SKILL.md"), text).unwrap();
        assert_eq!(skill.name, "demo");
        assert_eq!(skill.description, "A demo skill");
        assert!(skill.body.contains("Do the thing"));
    }

    #[test]
    fn builtins_have_names_and_bodies() {
        for s in builtin_skills() {
            assert!(!s.name.is_empty() && !s.description.is_empty() && !s.body.is_empty());
        }
    }
}
