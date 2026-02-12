use std::collections::VecDeque;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::AgentError,
    schemas::agent::{AgentAction, AgentEvent, AgentFinish},
};

use super::prompt::FORMAT_INSTRUCTIONS;

#[derive(Debug, Deserialize)]
struct AgentOutput {
    action: String,
    action_input: String,
}

pub struct ChatOutputParser {}
impl ChatOutputParser {
    pub fn new() -> Self {
        Self {}
    }
}

impl ChatOutputParser {
    pub fn parse(&self, text: &str) -> Result<AgentEvent, AgentError> {
        log::debug!("Parsing to Agent Action: {}", text);
        // Try markdown code block first, then raw JSON object (e.g. model returns plain JSON)
        let value = parse_json_markdown(text).or_else(|| {
            let trimmed = text.trim();
            if trimmed.starts_with('{') {
                parse_partial_json(trimmed, false)
            } else {
                None
            }
        });
        match value {
            Some(value) => {
                // Extract action and action_input from the parsed JSON
                let action = value
                    .get("action")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AgentError::OtherError("Missing or invalid 'action' field".to_string())
                    })?
                    .to_string();

                // Handle action_input: it can be a string or an object
                // If it's an object, serialize it to a string
                let action_input = match value.get("action_input") {
                    Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
                    Some(v) => serde_json::to_string(v)?,
                    None => {
                        return Err(AgentError::OtherError(
                            "Missing 'action_input' field".to_string(),
                        ))
                    }
                };

                if action == "Final Answer" {
                    Ok(AgentEvent::Finish(AgentFinish {
                        output: action_input,
                    }))
                } else {
                    Ok(AgentEvent::Action(vec![AgentAction {
                        tool: action,
                        tool_input: action_input,
                        log: text.to_string(),
                    }]))
                }
            }
            None => {
                log::debug!("No JSON found or malformed JSON in text: {}", text);
                Ok(AgentEvent::Finish(AgentFinish {
                    output: text.to_string(),
                }))
            }
        }
    }

    pub fn get_format_instructions(&self) -> &str {
        FORMAT_INSTRUCTIONS
    }
}

fn parse_partial_json(s: &str, strict: bool) -> Option<Value> {
    // First, attempt to parse the string as-is.
    match serde_json::from_str::<Value>(s) {
        Ok(val) => return Some(val),
        Err(_) if !strict => (),
        Err(_) => return None,
    }

    let mut new_s = String::new();
    let mut stack: VecDeque<char> = VecDeque::new();
    let mut is_inside_string = false;
    let mut escaped = false;

    for char in s.chars() {
        match char {
            '"' if !escaped => is_inside_string = !is_inside_string,
            '{' if !is_inside_string => stack.push_back('}'),
            '[' if !is_inside_string => stack.push_back(']'),
            '}' | ']' if !is_inside_string => {
                if let Some(c) = stack.pop_back() {
                    if c != char {
                        return None; // Mismatched closing character
                    }
                } else {
                    return None; // Unbalanced closing character
                }
            }
            '\\' if is_inside_string => escaped = !escaped,
            _ => escaped = false,
        }
        new_s.push(char);
    }

    // Close any open structures.
    while let Some(c) = stack.pop_back() {
        new_s.push(c);
    }

    // Attempt to parse again.
    serde_json::from_str(&new_s).ok()
}

fn parse_json_markdown(json_markdown: &str) -> Option<Value> {
    let re = Regex::new(r"```(?:json)?\s*([\s\S]+?)\s*```").unwrap();
    if let Some(caps) = re.captures(json_markdown) {
        if let Some(json_str) = caps.get(1) {
            return parse_partial_json(json_str.as_str(), false);
        }
    }
    None
}
