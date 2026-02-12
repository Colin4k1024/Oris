//! Repair message history when tool calls are interrupted (dangling tool calls).
//!
//! See [harness – Dangling tool call repair](https://docs.langchain.com/oss/python/deepagents/harness#dangling-tool-call-repair).

use crate::schemas::messages::{Message, MessageType};

const CANCELLED_CONTENT: &str = "Tool call was cancelled or interrupted.";

/// Ensures every AIMessage that has `tool_calls` has a matching number of following ToolMessages.
/// If some are missing (e.g. execution was interrupted), inserts synthetic ToolMessages
/// so the history remains valid for the next turn.
pub fn repair_dangling_tool_calls(messages: Vec<Message>) -> Vec<Message> {
    let mut out = Vec::with_capacity(messages.len());
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        let (needed, ids) = if msg.message_type == MessageType::AIMessage {
            extract_tool_call_count_and_ids(msg)
        } else {
            (0, Vec::new())
        };

        if needed == 0 {
            out.push(msg.clone());
            i += 1;
            continue;
        }

        let mut count = 0usize;
        let mut j = i + 1;
        while j < messages.len()
            && count < needed
            && messages[j].message_type == MessageType::ToolMessage
        {
            count += 1;
            j += 1;
        }

        out.push(msg.clone());
        for k in 0..count {
            out.push(messages[i + 1 + k].clone());
        }
        for k in count..needed {
            let id = ids
                .get(k)
                .cloned()
                .unwrap_or_else(|| format!("call_cancelled_{}", k));
            out.push(Message::new_tool_message(CANCELLED_CONTENT, id));
        }
        i = j;
    }
    out
}

fn extract_tool_call_count_and_ids(msg: &Message) -> (usize, Vec<String>) {
    let Some(ref v) = msg.tool_calls else {
        return (0, Vec::new());
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return (0, Vec::new()),
    };
    let n = arr.len();
    let ids: Vec<String> = arr
        .iter()
        .filter_map(|e| e.get("id").and_then(|id| id.as_str()).map(String::from))
        .collect();
    if ids.len() < n {
        let mut full = ids;
        for k in full.len()..n {
            full.push(format!("call_cancelled_{}", k));
        }
        (n, full)
    } else {
        (n, ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repair_inserts_missing_tool_messages() {
        let messages = vec![
            Message::new_human_message("run X"),
            Message::new_ai_message("").with_tool_calls(
                json!([{"id": "call_1", "name": "tool_a"}, {"id": "call_2", "name": "tool_b"}]),
            ),
        ];
        let repaired = repair_dangling_tool_calls(messages);
        assert_eq!(repaired.len(), 4);
        assert_eq!(repaired[0].message_type, MessageType::HumanMessage);
        assert_eq!(repaired[1].message_type, MessageType::AIMessage);
        assert_eq!(repaired[2].message_type, MessageType::ToolMessage);
        assert_eq!(repaired[2].content, CANCELLED_CONTENT);
        assert_eq!(repaired[2].id.as_deref(), Some("call_1"));
        assert_eq!(repaired[3].message_type, MessageType::ToolMessage);
        assert_eq!(repaired[3].id.as_deref(), Some("call_2"));
    }

    #[test]
    fn repair_leaves_complete_sequence_unchanged() {
        let messages = vec![
            Message::new_ai_message("").with_tool_calls(json!([{"id": "c1"}])),
            Message::new_tool_message("ok", "c1"),
        ];
        let repaired = repair_dangling_tool_calls(messages);
        assert_eq!(repaired.len(), 2);
        assert_eq!(repaired[1].content, "ok");
    }
}
