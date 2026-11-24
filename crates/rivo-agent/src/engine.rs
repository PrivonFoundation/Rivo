//! The agent turn loop. It runs on a worker thread and reports progress through an
//! `mpsc` channel; the UI only folds the events into its state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Sender};
use std::thread::{self, JoinHandle};

use crate::context;
use crate::model::{ChatInput, Event, EventKind, Message, Role, ToolCall};
use crate::provider::{self, StreamEvent};
use crate::settings::RunConfig;
use crate::tools::{ToolContext, all_tools};

pub fn spawn(
    session_id: String,
    turn: u64,
    cancel: Arc<AtomicBool>,
    input: ChatInput,
    run: RunConfig,
    events: Sender<Event>,
) -> JoinHandle<()> {
    thread::spawn(move || execute(session_id, turn, cancel, input, run, events))
}

fn execute(
    session_id: String,
    turn: u64,
    cancel: Arc<AtomicBool>,
    input: ChatInput,
    run: RunConfig,
    events: Sender<Event>,
) {
    let send = |kind: EventKind| {
        let _ = events.send(Event { session: session_id.clone(), turn, kind });
    };

    if let Some(problem) = run.provider.problem() {
        send(EventKind::Error(problem.into()));
        send(EventKind::Done);
        return;
    }

    let provider = provider::make(&run);
    let model = run.provider.model.clone();
    let tools: Vec<_> = all_tools().into_iter().filter(|t| run.settings.tool_enabled(t.name())).collect();
    let ChatInput { messages, cwd } = input;
    let ctx = ToolContext::new(cwd, run.settings.mode, run.skills.clone());
    let mut history = messages;

    for step in 0..run.settings.max_steps {
        if cancel.load(Ordering::Relaxed) { break; }

        // Fold old turns into a marker before they crowd out the useful context. The
        // `needs_compaction` check keeps this allocation-free while the history is small.
        if run.settings.auto_compact
            && context::needs_compaction(&history, run.settings.keep_recent, run.model.context_window)
        {
            let (compacted, removed) = context::compact(&history, run.settings.keep_recent, run.model.context_window);
            if removed > 0 {
                history = compacted;
                send(EventKind::ContextCompacted { removed });
            }
        }
        let usage = run.context(&history);
        send(EventKind::Usage { used: usage.used, window: usage.window });

        let mut turn_text = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut index = 0usize;

        let result = provider.chat(&run, &history, &tools, &mut |ev| {
            if cancel.load(Ordering::Relaxed) { return; }
            match ev {
                StreamEvent::Delta(text) => {
                    turn_text.push_str(&text);
                    send(EventKind::Delta(text));
                }
                StreamEvent::ToolCall { id, name, arguments } => {
                    let call = ToolCall::new(if id.is_empty() { format!("call-{index}") } else { id }, name, arguments);
                    index += 1;
                    send(EventKind::ToolStarted(call.clone()));
                    calls.push(call);
                }
                StreamEvent::Usage(usage) => {
                    send(EventKind::ModelUsage { model: model.clone(), usage });
                }
                StreamEvent::Done => {}
            }
        });

        if result.is_ok() {
            send(EventKind::ModelStep { model: model.clone() });
        }
        if let Err(e) = result {
            if !cancel.load(Ordering::Relaxed) { send(EventKind::Error(e)); }
            break;
        }

        let text = if step > 0 && !turn_text.is_empty() { format!("\n\n{turn_text}") } else { turn_text };
        let mut assistant = Message::new(Role::Assistant, text);
        assistant.tools = calls;
        history.push(assistant);

        let Some(last) = history.last_mut() else { break };
        if last.tools.is_empty() { break; }
        for call in last.tools.iter_mut() {
            if cancel.load(Ordering::Relaxed) { break; }
            let tool = tools.iter().find(|t| t.name() == call.name);
            let result = match serde_json::from_str(&call.arguments) {
                Ok(args) => match tool {
                    Some(t) => t.execute(&ctx, &args),
                    None => Err(format!("unknown or disabled tool: {}", call.name)),
                },
                Err(e) => Err(format!("invalid tool arguments: {e}")),
            };
            let ok = result.is_ok();
            let text = result.unwrap_or_else(|e| e);
            send(EventKind::ToolFinished { id: call.id.clone(), result: text.clone(), ok });
            call.result = Some(text);
            call.ok = ok;
            call.finished = true;
        }
    }

    send(EventKind::Done);
}
