//! MCP JSON-RPC surface for complete governed experience operations.

use crate::control_plane::{ExperienceControlPlane, ExperienceSearchQuery};
use crate::key_service::{KeyId, KeyStore};
use oris_experience_contract::{CapsuleV1, ExperienceBundleV1, ExperienceScope, UsageReceiptV1};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

pub const READ_TOOL_NAMES: &[&str] = &[
    "oris_experience_search",
    "oris_experience_get",
    "oris_experience_project_skill",
];
pub const WRITE_TOOL_NAMES: &[&str] = &[
    "oris_experience_propose",
    "oris_experience_begin_use",
    "oris_experience_record_outcome",
    "oris_experience_register_public_key",
    "oris_experience_revoke_public_key",
];
pub const GOVERN_TOOL_NAMES: &[&str] = &["oris_experience_promote", "oris_experience_revoke"];
pub const ADMIN_TOOL_NAMES: &[&str] = &[
    "oris_experience_list_api_keys",
    "oris_experience_create_api_key",
    "oris_experience_rotate_api_key",
    "oris_experience_revoke_api_key",
    "oris_experience_list_public_keys",
];
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

#[derive(Debug, Clone, Default)]
pub struct McpAuth {
    pub agent_id: String,
    pub can_read: bool,
    pub can_write: bool,
    pub can_govern: bool,
    pub can_admin: bool,
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
    key_store: Option<Arc<Mutex<KeyStore>>>,
}

impl ExperienceMcpServer {
    pub fn new(store: Arc<Mutex<ExperienceControlPlane>>) -> Self {
        Self {
            store,
            key_store: None,
        }
    }

    pub fn with_key_store(mut self, key_store: Arc<Mutex<KeyStore>>) -> Self {
        self.key_store = Some(key_store);
        self
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
                "protocolVersion": negotiated_protocol_version(&params),
                "capabilities": {
                    "tools": {"listChanged": false},
                    "resources": {"subscribe": false, "listChanged": false},
                    "prompts": {"listChanged": false}
                },
                "serverInfo": {
                    "name": "oris-experience",
                    "title": "Oris Experience Control Plane",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "Treat every Oris Gene as a suggestion. Preserve the caller's sandbox and approvals, call begin_use before adoption, validate in the caller environment, and record only evidence-backed outcomes."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => {
                let cursor = params.get("cursor").and_then(Value::as_str);
                if cursor.is_some() {
                    return Err(invalid_params("invalid tools cursor"));
                }
                Ok(json!({"tools": tools_for_auth(auth)}))
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
                Ok(json!({"resources": [
                    {"uri":"oris://capabilities","name":"Oris Experience capabilities","description":"The exact operations visible to this caller and their required scopes","mimeType":"application/json"},
                    {"uri":"oris://instructions","name":"Oris Experience lifecycle instructions","description":"Safe cross-agent reuse and contribution workflow","mimeType":"text/markdown"}
                ]}))
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
                if uri == "oris://instructions" {
                    return Ok(
                        json!({"contents":[{"uri":uri,"mimeType":"text/markdown","text":experience_instructions()}]}),
                    );
                }
                let value = if uri == "oris://capabilities" {
                    Ok(capability_manifest(auth))
                } else if let Some(id) = uri.strip_prefix("oris://genes/") {
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
            "prompts/list" => {
                require(auth.can_read, "experience:read")?;
                let mut prompts = vec![
                    json!({"name":"oris_experience_reuse","title":"Reuse verified Oris experience","description":"Search, inspect, adopt, validate, and record one applicable Gene","arguments":[{"name":"task","description":"Current engineering task or failure","required":true},{"name":"project_id","description":"Optional project boundary","required":false}]}),
                ];
                if auth.can_write {
                    prompts.push(json!({"name":"oris_experience_contribute","title":"Contribute verified Oris experience","description":"Propose a reusable procedure after a terminal evidence-backed result","arguments":[{"name":"result","description":"Validated result to extract into a reusable procedure","required":true}]}));
                }
                if auth.can_govern {
                    prompts.push(json!({"name":"oris_experience_govern","title":"Govern Oris experience","description":"Review evidence before promotion, revocation, or quarantine","arguments":[{"name":"gene_id","description":"Gene to review","required":true}]}));
                }
                Ok(json!({"prompts":prompts}))
            }
            "prompts/get" => {
                require(auth.can_read, "experience:read")?;
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid_params("missing prompt name"))?;
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                prompt(name, &arguments, auth)
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
            "oris_experience_project_skill" | "oris.experience.project_skill" => {
                require(auth.can_read, "experience:read")?;
                let id = required_str(&args, "id")?;
                let version = args
                    .get("version")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32);
                serde_json::to_value(
                    self.store
                        .lock()
                        .await
                        .skill_projection(id, version)
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
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
            "oris_experience_register_public_key" | "oris.experience.register_public_key" => {
                require(auth.can_write, "experience:write")?;
                let sender_id = required_str(&args, "sender_id")?;
                if sender_id != auth.agent_id {
                    return Err(forbidden("public key sender_id does not match caller"));
                }
                let key_store = self.key_store()?;
                let public_key = {
                    let key_store = key_store.lock().await;
                    key_store
                        .register_public_key(sender_id, required_str(&args, "public_key_hex")?)
                        .map_err(store_error)?
                };
                serde_json::to_value(public_key).map_err(json_error)?
            }
            "oris_experience_revoke_public_key" | "oris.experience.revoke_public_key" => {
                require(auth.can_write, "experience:write")?;
                let sender_id = required_str(&args, "sender_id")?;
                if sender_id != auth.agent_id {
                    return Err(forbidden("public key sender_id does not match caller"));
                }
                self.key_store()?
                    .lock()
                    .await
                    .revoke_public_key(sender_id)
                    .map_err(store_error)?;
                json!({"sender_id":sender_id,"revoked":true})
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
            "oris_experience_list_api_keys" | "oris.experience.list_api_keys" => {
                require(auth.can_admin, "experience:admin")?;
                serde_json::to_value(
                    self.key_store()?
                        .lock()
                        .await
                        .list_keys()
                        .map_err(store_error)?,
                )
                .map_err(json_error)?
            }
            "oris_experience_create_api_key" | "oris.experience.create_api_key" => {
                require(auth.can_admin, "experience:admin")?;
                let scopes = optional_string_vec(&args, "scopes")?;
                if scopes.iter().any(|scope| scope == "experience:govern") && !auth.can_govern {
                    return Err(forbidden(
                        "only governance callers can delegate experience:govern",
                    ));
                }
                let key_store = self.key_store()?;
                let (api_key, info) = key_store
                    .lock()
                    .await
                    .create_key_with_scopes(
                        required_str(&args, "agent_id")?,
                        args.get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        args.get("ttl_days").and_then(Value::as_i64),
                        scopes,
                    )
                    .map_err(store_error)?;
                json!({"api_key":api_key,"key":info})
            }
            "oris_experience_rotate_api_key" | "oris.experience.rotate_api_key" => {
                require(auth.can_admin, "experience:admin")?;
                let key_id = KeyId(required_str(&args, "key_id")?.to_owned());
                let (api_key, info) = self
                    .key_store()?
                    .lock()
                    .await
                    .rotate_key(&key_id, args.get("ttl_days").and_then(Value::as_i64))
                    .map_err(store_error)?;
                json!({"api_key":api_key,"key":info})
            }
            "oris_experience_revoke_api_key" | "oris.experience.revoke_api_key" => {
                require(auth.can_admin, "experience:admin")?;
                let key_id = KeyId(required_str(&args, "key_id")?.to_owned());
                self.key_store()?
                    .lock()
                    .await
                    .revoke_key(&key_id)
                    .map_err(store_error)?;
                json!({"key_id":key_id.0,"revoked":true})
            }
            "oris_experience_list_public_keys" | "oris.experience.list_public_keys" => {
                require(auth.can_admin, "experience:admin")?;
                serde_json::to_value(
                    self.key_store()?
                        .lock()
                        .await
                        .list_public_keys()
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

    fn key_store(&self) -> Result<Arc<Mutex<KeyStore>>, JsonRpcError> {
        self.key_store
            .clone()
            .ok_or_else(|| forbidden("key management is unavailable on this transport"))
    }
}

fn tools_for_auth(auth: &McpAuth) -> Vec<Value> {
    let mut tools = Vec::new();
    if auth.can_read {
        tools.extend(read_tools());
    }
    if auth.can_write {
        tools.extend(write_tools());
    }
    if auth.can_govern {
        tools.extend(governance_tools());
    }
    if auth.can_admin {
        tools.extend(admin_tools());
    }
    tools
}

fn read_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_search",
            "Find applicable, safe experience suggestions",
            json!({"type":"object","properties":{"text":{"type":"string"},"task_category":{"type":"string"},"project_id":{"type":"string"},"tenant_id":{"type":"string"},"available_tools":{"type":"array","items":{"type":"string"}},"environment":{"type":"object"},"limit":{"type":"integer"},"cursor":{"type":"string"}}}),
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_get",
            "Get a complete Gene bundle, evidence, and optional portable Skill",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"},"version":{"type":"integer"},"include_skill_projection":{"type":"boolean"}}}),
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_project_skill",
            "Project a Gene into a portable Agent Skill without installing it",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"},"version":{"type":"integer"}}}),
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
    ]
}

fn write_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_propose",
            "Submit a validated local candidate bundle",
            json!({"type":"object","required":["bundle"],"properties":{"bundle":{"type":"object"}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_begin_use",
            "Start a traceable Gene use",
            json!({"type":"object","required":["gene_id","gene_version","run_id","task_context_hash"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"run_id":{"type":"string"},"task_context_hash":{"type":"string"}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_record_outcome",
            "Record verified adoption outcome and optional evidence capsule",
            json!({"type":"object","required":["receipt"],"properties":{"receipt":{"type":"object"},"capsule":{"type":"object"}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_register_public_key",
            "Register or rotate the caller's Ed25519 verification key",
            json!({"type":"object","required":["sender_id","public_key_hex"],"properties":{"sender_id":{"type":"string"},"public_key_hex":{"type":"string","minLength":64,"maxLength":64}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_revoke_public_key",
            "Revoke the caller's Ed25519 verification key",
            json!({"type":"object","required":["sender_id"],"properties":{"sender_id":{"type":"string"}}}),
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}),
        ),
    ]
}
fn governance_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_promote",
            "Publish a stable Gene to an approved scope",
            json!({"type":"object","required":["gene_id","gene_version","scope"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"scope":{"enum":["local","project","tenant","team","network"]}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_revoke",
            "Revoke or quarantine a Gene version",
            json!({"type":"object","required":["gene_id","gene_version"],"properties":{"gene_id":{"type":"string"},"gene_version":{"type":"integer"},"quarantine":{"type":"boolean"}}}),
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}),
        ),
    ]
}

fn admin_tools() -> Vec<Value> {
    vec![
        tool(
            "oris_experience_list_api_keys",
            "List API key metadata without secret values",
            json!({"type":"object","properties":{}}),
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_create_api_key",
            "Create an explicitly scoped Experience Repository API key",
            json!({"type":"object","required":["agent_id","scopes"],"properties":{"agent_id":{"type":"string"},"description":{"type":"string"},"ttl_days":{"type":"integer"},"scopes":{"type":"array","items":{"type":"string"}}}}),
            json!({"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_rotate_api_key",
            "Rotate an API key and return its replacement secret once",
            json!({"type":"object","required":["key_id"],"properties":{"key_id":{"type":"string"},"ttl_days":{"type":"integer"}}}),
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_revoke_api_key",
            "Revoke an Experience Repository API key",
            json!({"type":"object","required":["key_id"],"properties":{"key_id":{"type":"string"}}}),
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}),
        ),
        tool(
            "oris_experience_list_public_keys",
            "List active Ed25519 public verification keys",
            json!({"type":"object","properties":{}}),
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}),
        ),
    ]
}
fn tool(name: &str, description: &str, input_schema: Value, annotations: Value) -> Value {
    json!({
        "name":name,
        "description":description,
        "inputSchema":input_schema,
        "outputSchema":{"type":"object","required":["data"],"properties":{"data":{}}},
        "annotations":annotations
    })
}

fn negotiated_protocol_version(params: &Value) -> &str {
    params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
        .unwrap_or("2025-06-18")
}

fn capability_manifest(auth: &McpAuth) -> Value {
    let operations = tools_for_auth(auth)
        .into_iter()
        .map(|tool| tool["name"].clone())
        .collect::<Vec<_>>();
    let granted_scopes = [
        auth.can_read.then_some("experience:read"),
        auth.can_write.then_some("experience:write"),
        auth.can_govern.then_some("experience:govern"),
        auth.can_admin.then_some("experience:admin"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let prompts = [
        auth.can_read.then_some("oris_experience_reuse"),
        (auth.can_read && auth.can_write).then_some("oris_experience_contribute"),
        (auth.can_read && auth.can_govern).then_some("oris_experience_govern"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    json!({
        "schema_version":"oris.agent-capabilities/v1",
        "server":"oris-experience",
        "agent_id":auth.agent_id,
        "granted_scopes":granted_scopes,
        "operations":operations,
        "resources":["oris://capabilities","oris://instructions","oris://genes/{id}","oris://capsules/{id}"],
        "prompts":prompts
    })
}

fn experience_instructions() -> &'static str {
    "# Oris Experience lifecycle\n\n1. Search with the real task, tools, environment, project, and tenant boundaries.\n2. Inspect the complete Gene and reject incompatible or quarantined suggestions.\n3. Call begin_use before applying any step.\n4. Preserve the host sandbox, approval policy, and repository instructions.\n5. Validate the result in the caller environment.\n6. Record the actual outcome; success requires test evidence.\n7. Propose only reusable procedures and never raw conversations, credentials, or user preferences.\n"
}

fn prompt(name: &str, arguments: &Value, auth: &McpAuth) -> Result<Value, JsonRpcError> {
    let text = match name {
        "oris_experience_reuse" => {
            let task = required_str(arguments, "task")?;
            let project = arguments
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("the current project");
            format!("For {project}, use the Oris lifecycle to find a safe, applicable Gene for this task: {task}. Begin a traceable use before adoption, preserve native approvals, validate the result, and record only the real evidence-backed outcome.")
        }
        "oris_experience_contribute" => {
            require(auth.can_write, "experience:write")?;
            let result = required_str(arguments, "result")?;
            format!("Extract a reusable procedure from this terminal validated result: {result}. Exclude raw conversation, credentials, preferences, and unverified claims. Build an ExperienceBundleV1 with applicability, negative boundaries, safety rules, validation checks, provenance, and evidence, then propose it as a local candidate.")
        }
        "oris_experience_govern" => {
            require(auth.can_govern, "experience:govern")?;
            let gene_id = required_str(arguments, "gene_id")?;
            format!("Review Gene {gene_id}, its Capsules, receipts, lifecycle thresholds, safety evidence, and target scope. Promote only when policy is satisfied; revoke or quarantine on safety failure. Never infer evidence that is not present.")
        }
        _ => return Err(invalid_params("unknown prompt name")),
    };
    Ok(
        json!({"description":"Oris governed experience workflow","messages":[{"role":"user","content":{"type":"text","text":text}}]}),
    )
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
fn optional_string_vec(value: &Value, key: &str) -> Result<Vec<String>, JsonRpcError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params(&format!("missing {key}")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid_params(&format!("{key} must contain strings")))
        })
        .collect()
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

    fn tool_names(value: &Value) -> Vec<String> {
        value["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    }

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
                    can_admin: false,
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
            can_admin: false,
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
                    can_admin: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(response.error.unwrap().code, -32003);
    }

    #[tokio::test]
    async fn discovery_is_trimmed_to_the_exact_granted_scopes() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )));
        let request = || JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        };
        let read_only = server
            .handle(
                request(),
                &McpAuth {
                    agent_id: "reader".into(),
                    can_read: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert_eq!(tool_names(&read_only), READ_TOOL_NAMES);
        let read_only_prompts = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(2)),
                    method: "prompts/list".into(),
                    params: json!({}),
                },
                &McpAuth {
                    agent_id: "reader".into(),
                    can_read: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert_eq!(read_only_prompts["prompts"].as_array().unwrap().len(), 1);
        assert!(!read_only_prompts
            .to_string()
            .contains("oris_experience_contribute"));

        let operator = server
            .handle(
                request(),
                &McpAuth {
                    agent_id: "operator".into(),
                    can_read: true,
                    can_write: true,
                    can_govern: true,
                    can_admin: false,
                },
            )
            .await
            .unwrap()
            .result
            .unwrap();
        let expected = READ_TOOL_NAMES
            .iter()
            .chain(WRITE_TOOL_NAMES)
            .chain(GOVERN_TOOL_NAMES)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(tool_names(&operator), expected);
    }

    #[tokio::test]
    async fn complete_primitives_and_latest_protocol_are_discoverable() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )));
        let auth = McpAuth {
            agent_id: "mcp-client".into(),
            can_read: true,
            can_write: true,
            can_govern: false,
            can_admin: false,
        };
        let initialize = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(1)),
                    method: "initialize".into(),
                    params: json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
                },
                &auth,
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert_eq!(initialize["protocolVersion"], "2025-11-25");
        assert!(initialize["capabilities"]["tools"].is_object());
        assert!(initialize["capabilities"]["resources"].is_object());
        assert!(initialize["capabilities"]["prompts"].is_object());

        let capabilities = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(2)),
                    method: "resources/read".into(),
                    params: json!({"uri":"oris://capabilities"}),
                },
                &auth,
            )
            .await
            .unwrap()
            .result
            .unwrap();
        let text = capabilities["contents"][0]["text"].as_str().unwrap();
        let manifest: Value = serde_json::from_str(text).unwrap();
        assert!(manifest["operations"]
            .as_array()
            .unwrap()
            .contains(&json!("oris_experience_project_skill")));

        let prompts = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(3)),
                    method: "prompts/list".into(),
                    params: json!({}),
                },
                &auth,
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert_eq!(prompts["prompts"].as_array().unwrap().len(), 2);
        assert!(!prompts.to_string().contains("oris_experience_govern"));
    }

    #[test]
    fn packaged_capability_manifest_matches_server_tools() {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../plugins/oris-experience/capabilities.json"
        ))
        .unwrap();
        let packaged = manifest["operations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|operation| operation["tool"].as_str().unwrap())
            .collect::<Vec<_>>();
        let expected = READ_TOOL_NAMES
            .iter()
            .chain(WRITE_TOOL_NAMES)
            .chain(GOVERN_TOOL_NAMES)
            .chain(ADMIN_TOOL_NAMES)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(packaged, expected);
    }

    #[tokio::test]
    async fn admin_tools_require_admin_scope_and_an_attached_key_store() {
        let server = ExperienceMcpServer::new(Arc::new(Mutex::new(
            ExperienceControlPlane::memory().unwrap(),
        )))
        .with_key_store(Arc::new(Mutex::new(KeyStore::memory().unwrap())));
        let admin = McpAuth {
            agent_id: "admin".into(),
            can_read: true,
            can_write: true,
            can_govern: true,
            can_admin: true,
        };
        let listed = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(1)),
                    method: "tools/list".into(),
                    params: json!({}),
                },
                &admin,
            )
            .await
            .unwrap()
            .result
            .unwrap();
        let expected = READ_TOOL_NAMES
            .iter()
            .chain(WRITE_TOOL_NAMES)
            .chain(GOVERN_TOOL_NAMES)
            .chain(ADMIN_TOOL_NAMES)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(tool_names(&listed), expected);

        let created = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(2)),
                    method: "tools/call".into(),
                    params: json!({
                        "name":"oris_experience_create_api_key",
                        "arguments":{"agent_id":"codex","scopes":["experience:read"]}
                    }),
                },
                &admin,
            )
            .await
            .unwrap()
            .result
            .unwrap();
        assert!(created["structuredContent"]["data"]["api_key"]
            .as_str()
            .is_some_and(|key| !key.is_empty()));

        let denied = server
            .handle(
                JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    id: Some(json!(3)),
                    method: "tools/call".into(),
                    params: json!({"name":"oris_experience_list_api_keys","arguments":{}}),
                },
                &McpAuth {
                    agent_id: "operator".into(),
                    can_read: true,
                    can_write: true,
                    can_govern: true,
                    can_admin: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(denied.error.unwrap().code, -32003);
    }
}
