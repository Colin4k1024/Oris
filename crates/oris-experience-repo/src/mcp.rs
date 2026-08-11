//! MCP 2025-compatible JSON-RPC surface for governed experience operations.

use crate::control_plane::{ExperienceControlPlane, ExperienceSearchQuery};
use oris_experience_contract::{CapsuleV1, ExperienceBundleV1, ExperienceScope, UsageReceiptV1};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct McpAuth {
    pub agent_id: String,
    pub can_read: bool,
    pub can_write: bool,
    pub can_govern: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone)]
pub struct ExperienceMcpServer {
    store: Arc<Mutex<ExperienceControlPlane>>,
}

impl ExperienceMcpServer {
    pub fn new(store: Arc<Mutex<ExperienceControlPlane>>) -> Self {
        Self { store }
    }

    pub async fn handle(&self, req: JsonRpcRequest, auth: &McpAuth) -> Option<JsonRpcResponse> {
        if req.jsonrpc != "2.0" {
            return Some(JsonRpcResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "invalid JSON-RPC version".into(),
                    data: None,
                }),
            });
        }
        if req.id.is_none() {
            // JSON-RPC notifications never receive a response.
            return None;
        }
        let id = req.id.clone();
        let result = self.dispatch(&req.method, req.params, auth).await;
        Some(match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(value),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(error),
            },
        })
    }

    async fn dispatch(
        &self,
        method: &str,
        params: Value,
        auth: &McpAuth,
    ) -> Result<Value, JsonRpcError> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": false}, "resources": {"subscribe": false, "listChanged": false}},
                "serverInfo": {"name": "oris-experience", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Experiences are suggestions. Validate them in the caller's sandbox and record the outcome."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => {
                require(auth.can_read, "experience:read")?;
                let cursor = params.get("cursor").and_then(Value::as_str);
                if cursor.is_some() && cursor != Some("governance") {
                    return Err(invalid_params("invalid tools cursor"));
                }
                let mut tools = ordinary_tools();
                if auth.can_govern {
                    tools.extend(governance_tools());
                }
                Ok(json!({"tools": tools}))
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("missing tool name"))?;
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.call_tool(name, args, auth).await
            }
            "resources/list" => {
                require(auth.can_read, "experience:read")?;
                Ok(json!({"resources": []}))
            }
            "resources/templates/list" => {
                require(auth.can_read, "experience:read")?;
                Ok(json!({"resourceTemplates": [
                    {"uriTemplate":"oris://genes/{id}","name":"Oris Gene","mimeType":"application/json"},
                    {"uriTemplate":"oris://capsules/{id}","name":"Oris Capsule","mimeType":"application/json"}
                ]}))
            }
            "resources/read" => {
                require(auth.can_read, "experience:read")?;
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("missing resource uri"))?;
                let value = if let Some(id) = uri.strip_prefix("oris://genes/") {
                    serde_json::to_value(
                        self.store
                            .lock()
                            .await
                            .bundle(id, None)
                            .map_err(store_error)?,
                    )
                } else if let Some(id) = uri.strip_prefix("oris://capsules/") {
                    serde_json::to_value(
                        self.store
                            .lock()
                            .await
                            .get_capsule(id)
                            .map_err(store_error)?,
                    )
                } else {
                    return Err(invalid_params("unsupported resource URI"));
                }
                .map_err(json_error)?;
                Ok(
                    json!({"contents":[{"uri":uri,"mimeType":"application/json","text":serde_json::to_string_pretty(&value).map_err(json_error)?}]}),
                )
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: "method not found".into(),
                data: Some(json!({"method":method})),
            }),
        }
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Value,
        auth: &McpAuth,
    ) -> Result<Value, JsonRpcError> {
        let value = match name {
            "oris_experience_search" | "oris.experience.search" => {
                require(auth.can_read, "experience:read")?;
                let query: ExperienceSearchQuery =
                    serde_json::from_value(args).map_err(json_error)?;
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .search(&query)
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_get" | "oris.experience.get" => {
                require(auth.can_read, "experience:read")?;
                let id = required_str(&args, "id")?;
                let version = args
                    .get("version")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32);
                let store = self.store.lock().await;
                let bundle = store.bundle(id, version).map_err(store_error)?;
                if args
                    .get("include_skill_projection")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    json!({"bundle":bundle,"skill_projection":store.skill_projection(id,version).map_err(store_error)?})
                } else {
                    serde_json::to_value(bundle).map_err(json_error)?
                }
            }
            "oris_experience_propose" | "oris.experience.propose" => {
                require(auth.can_write, "experience:write")?;
                let bundle: ExperienceBundleV1 =
                    serde_json::from_value(args.get("bundle").cloned().unwrap_or(args))
                        .map_err(json_error)?;
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .propose(&bundle)
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_begin_use" | "oris.experience.begin_use" => {
                require(auth.can_write, "experience:write")?;
                let id = required_str(&args, "gene_id")?;
                let version = required_u32(&args, "gene_version")?;
                let run_id = required_str(&args, "run_id")?;
                let context = required_str(&args, "task_context_hash")?;
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .begin_use(id, version, &auth.agent_id, run_id, context)
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_record_outcome" | "oris.experience.record_outcome" => {
                require(auth.can_write, "experience:write")?;
                let receipt: UsageReceiptV1 = serde_json::from_value(
                    args.get("receipt")
                        .cloned()
                        .ok_or_else(|| invalid_params("missing receipt"))?,
                )
                .map_err(json_error)?;
                if !auth.agent_id.is_empty() && receipt.agent_id != auth.agent_id {
                    return Err(forbidden("receipt agent_id does not match caller"));
                }
                let capsule: Option<CapsuleV1> = args
                    .get("capsule")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(json_error)?;
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .record_outcome(&receipt, capsule.as_ref())
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_promote" | "oris.experience.promote" => {
                require(auth.can_govern, "experience:govern")?;
                let scope: ExperienceScope = serde_json::from_value(
                    args.get("scope")
                        .cloned()
                        .ok_or_else(|| invalid_params("missing scope"))?,
                )
                .map_err(json_error)?;
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .promote(
                            required_str(&args, "gene_id")?,
                            required_u32(&args, "gene_version")?,
                            scope,
                            true,
                        )
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_revoke" | "oris.experience.revoke" => {
                require(auth.can_govern, "experience:govern")?;
                let quarantine = args
                    .get("quarantine")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .revoke(
                            required_str(&args, "gene_id")?,
                            required_u32(&args, "gene_version")?,
                            quarantine,
                            true,
                        )
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            _ => {
                return Err(JsonRpcError {
                    code: -32601,
                    message: "unknown tool".into(),
                    data: Some(json!({"tool":name})),
                })
            }
        };
        // MCP requires `structuredContent` to be a JSON object. Some Oris tools
        // naturally return arrays (notably search), so keep one stable envelope
        // across every tool instead of emitting a transport-invalid top-level
        // array that strict clients such as Claude Code reject.
        Ok(
            json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).map_err(json_error)?}],"structuredContent":{"data":value},"isError":false}),
        )
    }
}

fn ordinary_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_search",
            "Find applicable, safe experience suggestions",
            json!({"type":"object","properties":{"text":{"type":"string"},"task_category":{"type":"string"},"project_id":{"type":"string"},"tenant_id":{"type":"string"},"available_tools":{"type":"array","items":{"type":"string"}},"environment":{"type":"object"},"limit":{"type":"integer"},"cursor":{"type":"string"}}}),
        ),
        tool(
            "oris_experience_get",
            "Get a complete Gene bundle, evidence, and optional portable Skill",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"},"version":{"type":"integer"},"include_skill_projection":{"type":"boolean"}}}),
        ),
        tool(
            "oris_experience_propose",
            "Submit a validated local candidate bundle",
            json!({"type":"object","required":["bundle"],"properties":{"bundle":{"type":"object"}}}),
        ),
        tool(
            "oris_experience_begin_use",
            "Start a traceable Gene use",
            json!({"type":"object","required":["gene_id","gene_version","run_id","task_context_hash"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"run_id":{"type":"string"},"task_context_hash":{"type":"string"}}}),
        ),
        tool(
            "oris_experience_record_outcome",
            "Record verified adoption outcome and optional evidence capsule",
            json!({"type":"object","required":["receipt"],"properties":{"receipt":{"type":"object"},"capsule":{"type":"object"}}}),
        ),
    ]
}
fn governance_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_promote",
            "Publish a stable Gene to an approved scope",
            json!({"type":"object","required":["gene_id","gene_version","scope"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"scope":{"enum":["local","project","tenant","team","network"]}}}),
        ),
        tool(
            "oris_experience_revoke",
            "Revoke or quarantine a Gene version",
            json!({"type":"object","required":["gene_id","gene_version"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"quarantine":{"type":"boolean"}}}),
        ),
    ]
}
fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":input_schema})
}
fn require(ok: bool, scope: &str) -> Result<(), JsonRpcError> {
    if ok {
        Ok(())
    } else {
        Err(forbidden(&format!("missing scope {scope}")))
    }
}
fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, JsonRpcError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params(&format!("missing {key}")))
}
fn required_u32(value: &Value, key: &str) -> Result<u32, JsonRpcError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .ok_or_else(|| invalid_params(&format!("missing {key}")))
}
fn invalid_params(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
        data: None,
    }
}
fn forbidden(message: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32003,
        message: message.into(),
        data: None,
    }
}
fn store_error(error: impl std::fmt::Display) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message: error.to_string(),
        data: None,
    }
}
fn json_error(error: impl std::fmt::Display) -> JsonRpcError {
    invalid_params(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn governance_tools_are_capability_trimmed() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )));
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        };
        let result = server
            .handle(
                req,
                &McpAuth {
                    agent_id: "agent".into(),
                    can_read: true,
                    can_write: true,
                    can_govern: false,
                },
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert!(!result.to_string().contains("oris_experience_promote"));
        assert!(result.to_string().contains("oris_experience_search"));
        assert!(!result.to_string().contains("oris.experience.search"));
    }

    #[tokio::test]
    async fn propose_then_search_works_through_tools_call() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )));
        let auth = McpAuth {
            agent_id: "codex".into(),
            can_read: true,
            can_write: true,
            can_govern: false,
        };
        let bundle: ExperienceBundleV1 = serde_json::from_str(include_str!(
            "../../../spec/experience/golden/experience-bundle-v1.json"
        ))
        .unwrap();
        let propose = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"oris.experience.propose","arguments":{"bundle":bundle}}),
        };
        assert!(server.handle(propose, &auth).await.unwrap().error.is_none());
        let search = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({"name":"oris.experience.search","arguments":{"text":"rust http timeout","available_tools":["cargo"],"environment":{"language":"rust"}}}),
        };
        let result = server.handle(search, &auth).await.unwrap().result.unwrap();
        assert!(result["structuredContent"].is_object());
        assert!(result["structuredContent"]["data"]
            .as_array()
            .is_some_and(|items| items.len() == 1));
        let get = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: json!({"name":"oris.experience.get","arguments":{"id":"gene-rust-timeout-retry","include_skill_projection":true}}),
        };
        let result = server.handle(get, &auth).await.unwrap().result.unwrap();
        assert_eq!(
            result["structuredContent"]["data"]["skill_projection"]["installable"],
            json!(false)
        );
    }

    #[tokio::test]
    async fn governance_call_is_rejected_even_if_tool_name_is_known() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )));
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"oris.experience.revoke","arguments":{"gene_id":"missing","gene_version":1}}),
        };
        let response = server
            .handle(
                request,
                &McpAuth {
                    agent_id: "agent".into(),
                    can_read: true,
                    can_write: true,
                    can_govern: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(response.error.unwrap().code, -32003);
    }
}
