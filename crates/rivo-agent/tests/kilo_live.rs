//! Live checks against Kilo's free gateway. Ignored by default: they use the public
//! free quota (200 req/hour per IP) and need network access.
//!
//! Run with: cargo test -p rivo-agent --test kilo_live -- --ignored --nocapture

use rivo_agent::{AgentSettings, ChatInput, EventKind, Message, ModelConfig, Protocol, ProviderConfig, Role, RunConfig};

fn run_config(provider: ProviderConfig) -> RunConfig {
    RunConfig::new(provider, ModelConfig::default(), AgentSettings::default(), std::path::Path::new("."))
}

#[test]
#[ignore = "needs network and uses the public free-tier quota"]
fn kilo_free_gateway_streams_a_tool_call() {
    let config = ProviderConfig::kilo_free("kilo-auto/free");
    let status = rivo_agent::provider::probe(&config, Protocol::OpenAiCompatible).expect("gateway /models should answer");
    assert!(status.contains("models available"), "{status}");

    let input = ChatInput {
        messages: vec![Message::new(
            Role::User,
            "Use the list_dir tool to list the workspace, then answer with one short sentence.",
        )],
        cwd: std::env::current_dir().unwrap(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    rivo_agent::engine::spawn(
        "kilo".into(),
        1,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        input,
        run_config(config),
        tx,
    )
    .join()
    .unwrap();
    let events: Vec<_> = rx.try_iter().collect();
    println!("{events:#?}");
    assert!(events.iter().any(|e| matches!(e.kind, EventKind::Delta(_))));
    assert!(events.iter().any(|e| matches!(e.kind, EventKind::ToolStarted(_))));
    assert!(matches!(events.last().unwrap().kind, EventKind::Done));
}
