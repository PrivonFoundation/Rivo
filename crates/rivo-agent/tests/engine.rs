//! An end-to-end turn with the local provider: real tools, real events, no UI and no network.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use rivo_agent::{AgentSettings, ChatInput, EventKind, Message, ModelConfig, ProviderConfig, Role, RunConfig};
use std::path::Path;

fn run_config(provider: ProviderConfig) -> RunConfig {
    RunConfig::new(provider, ModelConfig::default(), AgentSettings::default(), Path::new("."))
}

#[test]
fn a_mock_turn_calls_a_tool_and_finishes() {
    let dir = std::env::temp_dir().join(format!("rivo-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "the answer is 42\n").unwrap();

    let input = ChatInput {
        messages: vec![Message::new(Role::User, "list the files in this workspace")],
        cwd: dir.clone(),
        model: "mock".into(),
    };
    let config = ProviderConfig::mock();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let handle = rivo_agent::engine::spawn("s1".into(), 1, cancel, input, run_config(config), tx);
    handle.join().unwrap();

    let events: Vec<_> = rx.try_iter().collect();
    assert!(events.iter().any(|e| matches!(e.kind, EventKind::Delta(_))), "streamed text");
    let started = events.iter().find_map(|e| match &e.kind {
        EventKind::ToolStarted(c) if c.name == "list_dir" => Some(c.clone()),
        _ => None,
    });
    assert!(started.is_some(), "called list_dir: {events:?}");
    assert!(events.iter().any(|e| matches!(&e.kind, EventKind::ToolFinished { ok: true, .. })), "tool succeeded");
    assert!(matches!(events.last().unwrap().kind, EventKind::Done), "turn completed");
    assert_eq!(events.last().unwrap().turn, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn errors_are_reported_when_no_api_key_is_set() {
    let config = ProviderConfig::custom("https://api.openai.com/v1".into(), String::new(), "gpt-4o-mini".into());
    let input = ChatInput {
        messages: vec![Message::new(Role::User, "hi")],
        cwd: PathBuf::from("."),
        model: config.model.clone(),
    };
    let (tx, rx) = mpsc::channel();
    rivo_agent::engine::spawn(
        "s2".into(),
        1,
        Arc::new(AtomicBool::new(false)),
        input,
        run_config(config),
        tx,
    )
    .join()
    .unwrap();
    let events: Vec<_> = rx.try_iter().collect();
    assert!(events.iter().any(|e| matches!(&e.kind, EventKind::Error(m) if m.contains("API key"))));
    assert!(matches!(events.last().unwrap().kind, EventKind::Done));
}

#[test]
fn a_cancelled_turn_stops_emitting_deltas() {
    let input = ChatInput {
        messages: vec![Message::new(Role::User, "write a very long document for me")],
        cwd: PathBuf::from("."),
        model: "mock".into(),
    };
    let config = ProviderConfig::mock();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(true)); // cancelled before the worker starts
    rivo_agent::engine::spawn("s3".into(), 2, cancel, input, run_config(config), tx).join().unwrap();
    let events: Vec<_> = rx.try_iter().collect();
    assert!(!events.iter().any(|e| matches!(e.kind, EventKind::Delta(_))));
    assert!(matches!(events.last().unwrap().kind, EventKind::Done));
}

#[test]
fn tool_results_stay_inside_the_workspace() {
    use rivo_agent::tools::{ToolContext, all_tools};
    let dir = std::env::temp_dir().join(format!("rivo-sandbox-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ToolContext::new(dir.clone(), rivo_agent::PermissionMode::Auto, Vec::new());
    let tools = all_tools();
    let write = tools.iter().find(|t| t.name() == "write_file").unwrap();
    assert!(write.execute(&ctx, &serde_json::json!({"path": "../escape.txt", "content": "x"})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
