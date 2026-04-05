//! IPC Request Handlers

use std::sync::Arc;

use tokio::sync::RwLock;

use oris_evo_ipc_protocol::{
    response::{ErrorCode, JsonRpcResponse, ResponseResult},
    JsonRpcRequest,
};
use tracing::{error, info};

use crate::error::{Error, Result};
use crate::pipeline::PipelineDriver;

/// Request handler for IPC
pub struct RequestHandler {
    /// Pipeline driver
    pipeline: Arc<RwLock<PipelineDriver>>,
}

impl RequestHandler {
    /// Create a new request handler
    pub fn new(pipeline: PipelineDriver) -> Self {
        Self {
            pipeline: Arc::new(RwLock::new(pipeline)),
        }
    }

    /// Handle a JSON-RPC request
    pub async fn handle(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let request_id = request.id;
        let method = request.method.clone();
        info!(method = %method, id = %request_id, "Handling request");

        let result = match method.as_str() {
            "evolve" => self.handle_evolve(request).await,
            "solidify" => self.handle_solidify(request).await,
            "revert" => self.handle_revert(request).await,
            "query" => self.handle_query(request).await,
            "list" => self.handle_list(request).await,
            "ping" => self.handle_ping(request).await,
            _ => Err(Error::Ipc(format!("Unknown method: {}", method))),
        };

        match result {
            Ok(response_result) => JsonRpcResponse::success(request_id, response_result),
            Err(e) => {
                error!(error = %e, "Request failed");
                JsonRpcResponse::error(
                    request_id,
                    match e.code() {
                        -32000 => ErrorCode::EvolutionError,
                        -32001 => ErrorCode::GeneNotFound,
                        -32002 => ErrorCode::ValidationFailed,
                        -32003 => ErrorCode::SandboxError,
                        -32004 => ErrorCode::SignatureError,
                        _ => ErrorCode::InternalError,
                    },
                    &e.to_string(),
                )
            }
        }
    }

    /// Handle evolve request
    async fn handle_evolve(&self, request: JsonRpcRequest) -> Result<ResponseResult> {
        let params = match &request.params {
            oris_evo_ipc_protocol::request::RequestParams::Evolve(p) => p.clone(),
            _ => {
                return Err(Error::Validation("Invalid params for evolve".to_string()));
            }
        };

        let pipeline = self.pipeline.read().await;
        let result = pipeline.evolve(params.signal).await?;

        Ok(ResponseResult::Evolve(result))
    }

    /// Handle solidify request
    async fn handle_solidify(&self, request: JsonRpcRequest) -> Result<ResponseResult> {
        let gene_id = match &request.params {
            oris_evo_ipc_protocol::request::RequestParams::Solidify(p) => p.gene_id,
            _ => {
                return Err(Error::Validation("Invalid params for solidify".to_string()));
            }
        };

        let pipeline = self.pipeline.read().await;
        let success = pipeline.solidify(gene_id).await?;

        Ok(ResponseResult::Solidify(oris_evo_ipc_protocol::response::SolidifyResponse {
            success,
            gene_id,
        }))
    }

    /// Handle revert request
    async fn handle_revert(&self, request: JsonRpcRequest) -> Result<ResponseResult> {
        let reason = match &request.params {
            oris_evo_ipc_protocol::request::RequestParams::Revert(p) => p.reason.clone(),
            _ => {
                return Err(Error::Validation("Invalid params for revert".to_string()));
            }
        };

        let pipeline = self.pipeline.read().await;
        let success = pipeline.revert(reason.gene_id, &reason.reason).await?;

        Ok(ResponseResult::Revert(oris_evo_ipc_protocol::response::RevertResponse {
            success,
            gene_id: reason.gene_id,
            message: format!("Reverted: {}", reason.reason),
        }))
    }

    /// Handle query request
    async fn handle_query(&self, request: JsonRpcRequest) -> Result<ResponseResult> {
        let query = match &request.params {
            oris_evo_ipc_protocol::request::RequestParams::Query(p) => p.query.clone(),
            _ => {
                return Err(Error::Validation("Invalid params for query".to_string()));
            }
        };

        let pipeline = self.pipeline.read().await;
        let genes = pipeline.query_genes(&query.pattern, query.limit.unwrap_or(10)).await?;

        Ok(ResponseResult::Query(genes))
    }

    /// Handle list request
    async fn handle_list(&self, request: JsonRpcRequest) -> Result<ResponseResult> {
        let (limit, offset) = match &request.params {
            oris_evo_ipc_protocol::request::RequestParams::List(p) => (p.limit.unwrap_or(100), p.offset.unwrap_or(0)),
            _ => {
                return Err(Error::Validation("Invalid params for list".to_string()));
            }
        };

        let pipeline = self.pipeline.read().await;
        let (genes, total) = pipeline.list_genes(limit, offset).await?;

        Ok(ResponseResult::List(oris_evo_ipc_protocol::response::ListResponse {
            total,
            genes,
        }))
    }

    /// Handle ping request
    async fn handle_ping(&self, _request: JsonRpcRequest) -> Result<ResponseResult> {
        Ok(ResponseResult::Ping(oris_evo_ipc_protocol::response::PingResponse {
            timestamp: chrono::Utc::now().to_rfc3339(),
            message: None,
            version: oris_evo_ipc_protocol::PROTOCOL_VERSION.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ping_handler() {
        // This would need a real pipeline to test properly
        // Skipping for now
    }
}
