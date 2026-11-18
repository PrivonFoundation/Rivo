//! Context-window accounting and compaction. Keeps a session usable after a long conversation:
//! old turns are folded into one summary marker instead of being silently dropped.

use crate::model::{Message, Role};

/// Rough token estimate (4 characters per token). Good enough for a budget indicator; the
/// provider's real usage is reported by `usage` events when available.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

pub fn conversation_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| estimate_tokens(&m.text) + 8).sum::<usize>()
        + messages.iter().flat_map(|m| m.tools.iter()).map(|c| {
            estimate_tokens(&c.arguments) + estimate_tokens(c.result.as_deref().unwrap_or("")) + 12
        }).sum::<usize>()
}

#[derive(Clone, Copy, Debug)]
pub struct ContextUsage {
    pub used: usize,
    pub window: usize,
    pub ratio: f32,
}

pub fn usage(messages: &[Message], window: usize) -> ContextUsage {
    let used = conversation_tokens(messages);
    ContextUsage { used, window, ratio: used as f32 / window.max(1) as f32 }
}

/// Whether compaction would actually change the history. Checking this first avoids cloning
/// the whole conversation on every tool-loop step when it is still below the threshold.
pub fn needs_compaction(messages: &[Message], keep_recent: usize, window: usize) -> bool {
    conversation_tokens(messages) >= window * 85 / 100 && messages.len() > keep_recent + 2
}

/// Keep the first user message (the task) and the `keep_recent` newest messages. Everything
/// between becomes a short marker, so the model still knows there was earlier work.
pub fn compact(messages: &[Message], keep_recent: usize, window: usize) -> (Vec<Message>, usize) {
    if !needs_compaction(messages, keep_recent, window) {
        return (messages.to_vec(), 0);
    }
    let anchor = messages.iter().find(|m| m.role == Role::User).cloned();
    let tail_start = messages.len().saturating_sub(keep_recent);
    let removed = tail_start.saturating_sub(anchor.is_some() as usize);
    let mut out = Vec::with_capacity(keep_recent + 2);
    if let Some(first) = anchor {
        out.push(first);
    }
    if removed > 0 {
        out.push(Message::new(
            Role::Assistant,
            format!("[Earlier context compacted: {removed} messages omitted. Continue from the recent turns above.]"),
        ));
    }
    out.extend_from_slice(&messages[tail_start..]);
    (out, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_keeps_the_task_and_the_tail() {
        let mut msgs = vec![Message::new(Role::User, "build a router")];
        for i in 0..80 {
            msgs.push(Message::new(Role::Assistant, format!("{}", i + 1).repeat(220)));
        }
        let (out, removed) = compact(&msgs, 4, 200);
        assert!(removed > 0);
        assert_eq!(out.first().unwrap().text, "build a router");
        assert!(out.iter().any(|m| m.text.starts_with("[Earlier context compacted")));
        assert_eq!(out.len(), 6);
    }
}
