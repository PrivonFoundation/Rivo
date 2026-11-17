//! Seed conversations used by the headless scenes and by a fresh app's "example" session.

use std::path::PathBuf;

use rivo_agent::{Message, Role, Session, ToolCall};

pub fn demo_session() -> Session {
    let mut s = Session::new(
        "demo",
        "Port the sidebar to Rune",
        "rivo",
        PathBuf::from("."),
    );
    s.messages.clear();

    s.push(Message::new(
        Role::User,
        "List the files in this workspace, then tell me how the agent is structured.",
    ));

    let mut a = Message::new(
        Role::Assistant,
        "I'll map the workspace first, then trace the agent loop from the UI down to the tools.",
    );
    a.tools.push(done(
        "call-1",
        "list_dir",
        r#"{"path":"."}"#,
        "  crates/\n  crates/rivo-agent/  (4096 bytes)\n  crates/rivo-app/  (4096 bytes)\n  vendor/rune/  (4096 bytes)\n  Cargo.toml  (612 bytes)\n  README.md  (2210 bytes)\n",
    ));
    s.push(a);

    s.push(Message::new(
        Role::Assistant,
        "The layout is small on purpose:\n\n- **`rivo-agent`** — tools, an OpenAI-compatible streaming provider and the turn loop. It knows nothing about the UI.\n- **`rivo-app`** — the Rune desktop shell, transcript and composer.\n\nThe engine emits events over a channel; `AppSpec::poll` folds them into the view state. That keeps every frame owned by Rune and every tool call testable without a window.",
    ));

    s.push(Message::new(Role::User, "Add a short overview to the README."));

    let mut b = Message::new(
        Role::Assistant,
        "Writing the section now. The write is confined to the workspace root, so the tool cannot touch files outside it.",
    );
    b.tools.push(done(
        "call-2",
        "write_file",
        r##"{"path":"docs/overview.md","content":"# Rivo\n\nA native coding agent built with Rune."}"##,
        "wrote 78 bytes to docs/overview.md",
    ));
    s.push(b);

    s.push(Message::new(
        Role::Assistant,
        "Added `docs/overview.md`. The architecture section now explains the channel boundary between the engine and the UI.",
    ));
    s
}

pub fn streaming_session() -> Session {
    let mut s = demo_session();
    s.title = "Streaming a tool result".into();
    s.busy = true;
    s.push(Message::new(Role::User, "Search for every `on_submit` handler."));

    let mut a = Message::new(Role::Assistant, "Searching the workspace for that symbol…");
    let mut call = ToolCall::new("call-3".into(), "search".into(), r#"{"query":"on_submit"}"#.into());
    call.finished = false;
    call.result = Some("crates/rivo-app/src/main.rs:214: .on_submit(|s: &mut Rivo, _| s.submit())\ncrates/rivo-app/src/views.rs:88: // composer\n".into());
    a.tools.push(call);
    s.push(a);
    s
}

fn done(id: &str, name: &str, args: &str, result: &str) -> ToolCall {
    let mut c = ToolCall::new(id.into(), name.into(), args.into());
    c.finished = true;
    c.result = Some(result.into());
    c.ok = true;
    c
}
