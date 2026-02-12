//! Browser Use tool: browser automation (navigate, click, type, scroll, get content).
//!
//! Requires the `browser-use` feature and Chrome/Chromium installed (or use
//! headless_chrome's bundled Chromium where available).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tools::Tool;

/// Input format for the tool: flat JSON with "action" and action-specific fields.
#[derive(Debug, Deserialize)]
struct BrowserUseInput {
    #[serde(rename = "action")]
    action_kind: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    press_enter: Option<bool>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    pixels: Option<i32>,
}

/// Tool that automates browser actions via headless Chrome.
///
/// Accepts JSON with `action` (navigate, click, type, scroll, get_content) and
/// action-specific parameters. Each call launches a browser, performs the action,
/// returns the result, then closes the browser.
pub struct BrowserUse;

impl BrowserUse {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BrowserUse {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "browser-use")]
fn run_browser_action(input: BrowserUseInput) -> Result<String, crate::error::ToolError> {
    use headless_chrome::Browser;

    let browser = Browser::default().map_err(|e| {
        crate::error::ToolError::ExecutionError(format!("Failed to launch browser: {}", e))
    })?;
    let tab = browser.new_tab().map_err(|e| {
        crate::error::ToolError::ExecutionError(format!("Failed to create tab: {}", e))
    })?;

    let action = input.action_kind.to_lowercase();
    let result = match action.as_str() {
        "navigate" => {
            let url = input.url.ok_or_else(|| {
                crate::error::ToolError::InvalidInputError("navigate requires 'url'".into())
            })?;
            tab.navigate_to(&url).map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Navigate failed: {}", e))
            })?;
            tab.wait_for_element("body").map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Wait for page failed: {}", e))
            })?;
            format!("Navigated to {}", url)
        }
        "click" => {
            let selector = input.selector.ok_or_else(|| {
                crate::error::ToolError::InvalidInputError("click requires 'selector'".into())
            })?;
            tab.wait_for_element(&selector)
                .map_err(|e| {
                    crate::error::ToolError::ExecutionError(format!("Element not found: {}", e))
                })?
                .click()
                .map_err(|e| {
                    crate::error::ToolError::ExecutionError(format!("Click failed: {}", e))
                })?;
            "Clicked.".to_string()
        }
        "type" => {
            let selector = input.selector.ok_or_else(|| {
                crate::error::ToolError::InvalidInputError("type requires 'selector'".into())
            })?;
            let text = input.text.unwrap_or_default();
            let element = tab.wait_for_element(&selector).map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Element not found: {}", e))
            })?;
            element.click().map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Click to focus failed: {}", e))
            })?;
            tab.type_str(&text).map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Type failed: {}", e))
            })?;
            if input.press_enter.unwrap_or(false) {
                tab.press_key("Enter").map_err(|e| {
                    crate::error::ToolError::ExecutionError(format!("Press Enter failed: {}", e))
                })?;
            }
            "Typed.".to_string()
        }
        "scroll" => {
            let pixels = input.pixels.unwrap_or(500);
            let dir = input.direction.as_deref().unwrap_or("down");
            let delta = if dir.eq_ignore_ascii_case("up") {
                -pixels
            } else {
                pixels
            };
            let js = format!("window.scrollBy(0, {});", delta);
            tab.evaluate(&js, false).map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Scroll failed: {}", e))
            })?;
            "Scrolled.".to_string()
        }
        "get_content" => {
            let js = input
                .selector
                .as_ref()
                .map(|sel| {
                    format!(
                        "(() => {{ const el = document.querySelector({:?}); return el ? el.innerText : document.body.innerText; }})()",
                        sel
                    )
                })
                .unwrap_or_else(|| "document.body.innerText".to_string());
            let result = tab.evaluate(&js, true).map_err(|e| {
                crate::error::ToolError::ExecutionError(format!("Get content failed: {}", e))
            })?;
            let text = result
                .value
                .as_ref()
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default();
            if text.len() > 50000 {
                format!("{}... (truncated)", &text[..50000])
            } else {
                text
            }
        }
        _ => {
            return Err(crate::error::ToolError::InvalidInputError(format!(
                "Unknown action: {}. Use one of: navigate, click, type, scroll, get_content",
                action
            )));
        }
    };

    Ok(result)
}

#[cfg(not(feature = "browser-use"))]
fn run_browser_action(_input: BrowserUseInput) -> Result<String, crate::error::ToolError> {
    Err(crate::error::ToolError::ConfigurationError(
        "browser-use feature is not enabled. Add 'browser-use' to your Cargo.toml features.".into(),
    ))
}

#[async_trait]
impl Tool for BrowserUse {
    fn name(&self) -> String {
        "Browser_Use".to_string()
    }

    fn description(&self) -> String {
        "Automates browser actions. Input must be JSON with 'action' and parameters. \
         Actions: navigate (url), click (selector), type (selector, text, optional press_enter), \
         scroll (optional direction: up/down, optional pixels), get_content (optional selector). \
         Example: {\"action\": \"navigate\", \"url\": \"https://example.com\"}."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "One of: navigate, click, type, scroll, get_content"
                },
                "url": { "type": "string", "description": "For navigate: URL to open" },
                "selector": { "type": "string", "description": "CSS selector for click, type, or get_content" },
                "text": { "type": "string", "description": "For type: text to type" },
                "press_enter": { "type": "boolean", "description": "For type: press Enter after typing" },
                "direction": { "type": "string", "description": "For scroll: up or down" },
                "pixels": { "type": "integer", "description": "For scroll: pixels to scroll" }
            },
            "required": ["action"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        match serde_json::from_str::<Value>(input) {
            Ok(v) => v,
            Err(_) => json!({ "action": "get_content" }),
        }
    }

    async fn run(&self, input: Value) -> Result<String, crate::error::ToolError> {
        let parsed: BrowserUseInput = serde_json::from_value(input).map_err(|e| {
            crate::error::ToolError::ParsingError(format!("Invalid BrowserUse input: {}", e))
        })?;

        // headless_chrome is synchronous; run in spawn_blocking to not block the async runtime
        let res = tokio::task::spawn_blocking(move || run_browser_action(parsed)).await;

        match res {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(crate::error::ToolError::ExecutionError(format!(
                "Browser task panicked: {}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_browser_use_name_and_description() {
        let tool = BrowserUse::new();
        assert_eq!(tool.name(), "Browser_Use");
        assert!(tool.description().contains("navigate"));
        assert!(tool.description().contains("click"));
    }

    #[tokio::test]
    async fn test_browser_use_parse_input() {
        let tool = BrowserUse::new();
        let v = tool
            .parse_input(r#"{"action": "navigate", "url": "https://example.com"}"#)
            .await;
        let obj = v.as_object().unwrap();
        assert_eq!(obj.get("action").and_then(|a| a.as_str()), Some("navigate"));
        assert_eq!(
            obj.get("url").and_then(|u| u.as_str()),
            Some("https://example.com")
        );
    }

    #[cfg(not(feature = "browser-use"))]
    #[tokio::test]
    async fn test_browser_use_run_without_feature_returns_error() {
        let tool = BrowserUse::new();
        let input = serde_json::json!({
            "action": "get_content",
            "url": null,
            "selector": null
        });
        let result = tool.run(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("browser-use") || err.to_string().contains("not enabled"));
    }
}
