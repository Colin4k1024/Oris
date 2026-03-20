//! Plugin categories and interfaces for the Oris kernel.
//!
//! ## K4 Implementation
//! - K4-a: Plugin categories - Tool, Checkpoint, Effect, Observer, Governor
//! - K4-b: Determinism contracts - Deterministic, NonDeterministic, EffectCapturing  
//! - K4-c: Resource limits for plugin execution sandbox
//! - K4-d: Version negotiation and dynamic registry

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::language_models::llm::LLM;
use crate::schemas::memory::BaseMemory;
use crate::tools::Tool;

#[derive(Error, Debug)]
#[error("Plugin error: {0}")]
pub struct PluginError(pub String);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginCompatibility {
    #[serde(default)]
    pub plugin_api_version: String,
    #[serde(default)]
    pub kernel_compat: String,
    #[serde(default)]
    pub schema_hash: Option<String>,
}

pub fn validate_plugin_compatibility(
    compat: &PluginCompatibility,
    kernel_version: &str,
) -> Result<(), PluginError> {
    if kernel_version.is_empty() || compat.kernel_compat.is_empty() {
        return Ok(());
    }
    let req = compat.kernel_compat.trim();
    if req.starts_with(">=") {
        let min = req.trim_start_matches(">=").trim();
        if kernel_version < min {
            return Err(PluginError(format!(
                "kernel version {} does not meet requirement {}",
                kernel_version, compat.kernel_compat
            )));
        }
    } else if req != kernel_version {
        return Err(PluginError(format!(
            "kernel version {} does not match {}",
            kernel_version, compat.kernel_compat
        )));
    }
    Ok(())
}

/// K4-b: Determinism contract - fail closed on NonDeterministic in deterministic mode
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum DeterminismContract {
    Deterministic,
    NonDeterministic,
    EffectCapturing,
}
impl Default for DeterminismContract {
    fn default() -> Self {
        DeterminismContract::NonDeterministic
    }
}
impl DeterminismContract {
    pub fn allowed_in_deterministic_mode(self) -> bool {
        matches!(
            self,
            DeterminismContract::Deterministic | DeterminismContract::EffectCapturing
        )
    }
}

/// Plugin metadata with K4-b determinism contract
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginMetadata {
    #[serde(default)]
    pub deterministic: bool,
    #[serde(default = "default_true")]
    pub side_effects: bool,
    #[serde(default)]
    pub replay_safe: bool,
    /// K4-b: Determinism contract
    #[serde(default)]
    pub determinism_contract: DeterminismContract,
}
fn default_true() -> bool {
    true
}
impl PluginMetadata {
    pub fn conservative() -> Self {
        Self {
            deterministic: false,
            side_effects: true,
            replay_safe: false,
            determinism_contract: DeterminismContract::NonDeterministic,
        }
    }
    pub fn pure() -> Self {
        Self {
            deterministic: true,
            side_effects: false,
            replay_safe: true,
            determinism_contract: DeterminismContract::Deterministic,
        }
    }
}

pub trait HasPluginMetadata: Send + Sync {
    fn plugin_metadata(&self) -> PluginMetadata {
        PluginMetadata::conservative()
    }
}
#[inline]
pub fn allow_in_replay(meta: &PluginMetadata) -> bool {
    meta.replay_safe
}
#[inline]
pub fn requires_sandbox(meta: &PluginMetadata) -> bool {
    meta.side_effects
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PluginExecutionMode {
    InProcess,
    IsolatedProcess,
    Remote,
}
impl PluginExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginExecutionMode::InProcess => "in_process",
            PluginExecutionMode::IsolatedProcess => "isolated_process",
            PluginExecutionMode::Remote => "remote",
        }
    }
}
#[inline]
pub fn route_to_execution_mode(meta: &PluginMetadata) -> PluginExecutionMode {
    if requires_sandbox(meta) {
        PluginExecutionMode::IsolatedProcess
    } else {
        PluginExecutionMode::InProcess
    }
}

/// K4-a: Plugin categories
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum PluginCategory {
    Node,
    Tool,
    Memory,
    LLMAdapter,
    Scheduler,
    Checkpoint,
    Effect,
    Observer,
    Governor,
}
impl PluginCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginCategory::Node => "node",
            PluginCategory::Tool => "tool",
            PluginCategory::Memory => "memory",
            PluginCategory::LLMAdapter => "llm_adapter",
            PluginCategory::Scheduler => "scheduler",
            PluginCategory::Checkpoint => "checkpoint",
            PluginCategory::Effect => "effect",
            PluginCategory::Observer => "observer",
            PluginCategory::Governor => "governor",
        }
    }
}

pub trait ToolPlugin: HasPluginMetadata {
    fn plugin_type(&self) -> &str;
    fn create_tool(&self, config: &Value) -> Result<Arc<dyn Tool>, PluginError>;
}
pub trait MemoryPlugin: HasPluginMetadata {
    fn plugin_type(&self) -> &str;
    fn create_memory(&self, config: &Value) -> Result<Arc<Mutex<dyn BaseMemory>>, PluginError>;
}
pub trait LLMAdapter: HasPluginMetadata {
    fn plugin_type(&self) -> &str;
    fn create_llm(&self, config: &Value) -> Result<Arc<dyn LLM>, PluginError>;
}
pub trait SchedulerPlugin: HasPluginMetadata {
    fn plugin_type(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
}

// K4-c: Resource Limits
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginResourceLimits {
    pub max_cpu_secs: Option<u64>,
    pub max_memory_bytes: Option<u64>,
    pub max_duration_ms: Option<u64>,
}
impl PluginResourceLimits {
    pub fn default_limits() -> Self {
        Self {
            max_cpu_secs: Some(60),
            max_memory_bytes: Some(512 * 1024 * 1024),
            max_duration_ms: Some(30_000),
        }
    }
    pub fn restrictive() -> Self {
        Self {
            max_cpu_secs: Some(10),
            max_memory_bytes: Some(128 * 1024 * 1024),
            max_duration_ms: Some(5_000),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceLimitViolation {
    pub plugin_id: String,
    pub limit_name: String,
    pub actual_value: u64,
    pub limit_value: u64,
}
impl std::fmt::Display for ResourceLimitViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Resource limit '{}' exceeded for plugin '{}': {} > {}",
            self.limit_name, self.plugin_id, self.actual_value, self.limit_value
        )
    }
}

// K4-d: Version Negotiation
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginCapability {
    pub name: String,
    pub version: String,
    pub category: PluginCategory,
    pub determinism_contract: DeterminismContract,
    #[serde(default)]
    pub kernel_version_range: String,
}
impl PluginCapability {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        category: PluginCategory,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            category,
            determinism_contract: DeterminismContract::default(),
            kernel_version_range: String::new(),
        }
    }
    pub fn with_determinism(mut self, c: DeterminismContract) -> Self {
        self.determinism_contract = c;
        self
    }
    pub fn with_kernel_version_range(mut self, r: impl Into<String>) -> Self {
        self.kernel_version_range = r.into();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VersionNegotiationResult {
    Compatible,
    IncompatibleVersion,
    IncompatibleKernel,
    CategoryMismatch,
    DeterminismViolation,
}

pub fn negotiate_version(
    pc: &PluginCapability,
    kv: &str,
    rc: PluginCategory,
    dm: bool,
) -> VersionNegotiationResult {
    if pc.category != rc {
        return VersionNegotiationResult::CategoryMismatch;
    }
    if dm && !pc.determinism_contract.allowed_in_deterministic_mode() {
        return VersionNegotiationResult::DeterminismViolation;
    }
    if !pc.kernel_version_range.is_empty() {
        let r = pc.kernel_version_range.trim();
        if r.starts_with(">=") {
            let min = r.trim_start_matches(">=").trim();
            if kv < min {
                return VersionNegotiationResult::IncompatibleKernel;
            }
        }
    }
    VersionNegotiationResult::Compatible
}

pub struct PluginRegistry {
    plugins: std::collections::HashMap<String, PluginRegistration>,
}
struct PluginRegistration {
    capability: PluginCapability,
    metadata: PluginMetadata,
    resource_limits: PluginResourceLimits,
}
impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: std::collections::HashMap::new(),
        }
    }
    pub fn register(
        &mut self,
        c: PluginCapability,
        m: PluginMetadata,
        r: PluginResourceLimits,
    ) -> Result<(), PluginError> {
        self.plugins.insert(
            format!("{}:{}", c.name, c.version),
            PluginRegistration {
                capability: c,
                metadata: m,
                resource_limits: r,
            },
        );
        Ok(())
    }
    pub fn unregister(&mut self, n: &str, v: &str) -> Result<(), PluginError> {
        self.plugins
            .remove(&format!("{}:{}", n, v))
            .ok_or_else(|| PluginError("not found".into()))?;
        Ok(())
    }
    pub fn get(
        &self,
        n: &str,
    ) -> Option<(&PluginCapability, &PluginMetadata, &PluginResourceLimits)> {
        self.plugins
            .get(n)
            .map(|r| (&r.capability, &r.metadata, &r.resource_limits))
    }
    pub fn contains(&self, n: &str) -> bool {
        self.plugins.contains_key(n)
    }
    pub fn validate(
        &self,
        n: &str,
        kv: &str,
        rc: PluginCategory,
        dm: bool,
    ) -> Result<VersionNegotiationResult, PluginError> {
        let (c, m, _) = self.get(n).ok_or_else(|| PluginError("not found".into()))?;
        if dm && !m.replay_safe {
            return Err(PluginError("not replay-safe".into()));
        }
        Ok(negotiate_version(c, kv, rc, dm))
    }
}
impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_category_as_str() {
        assert_eq!(PluginCategory::Checkpoint.as_str(), "checkpoint");
    }
    #[test]
    fn determinism_contract_allowed() {
        assert!(DeterminismContract::Deterministic.allowed_in_deterministic_mode());
        assert!(!DeterminismContract::NonDeterministic.allowed_in_deterministic_mode());
    }
    #[test]
    fn plugin_resource_limits_default() {
        let l = PluginResourceLimits::default_limits();
        assert_eq!(l.max_cpu_secs, Some(60));
    }
    #[test]
    fn negotiate_version_compatible() {
        let c = PluginCapability::new("t", "1", PluginCategory::Tool)
            .with_kernel_version_range(">=0.2");
        assert_eq!(
            negotiate_version(&c, "0.2.7", PluginCategory::Tool, false),
            VersionNegotiationResult::Compatible
        );
    }
    #[test]
    fn negotiate_version_deterministic_block() {
        let c = PluginCapability::new("t", "1", PluginCategory::Tool)
            .with_determinism(DeterminismContract::NonDeterministic);
        assert_eq!(
            negotiate_version(&c, "0.2", PluginCategory::Tool, true),
            VersionNegotiationResult::DeterminismViolation
        );
    }
    #[test]
    fn registry_register_get() {
        let mut r = PluginRegistry::new();
        r.register(
            PluginCapability::new("t", "1", PluginCategory::Tool),
            PluginMetadata::default(),
            PluginResourceLimits::default(),
        )
        .unwrap();
        assert!(r.contains("t:1"));
    }
    #[test]
    fn registry_validate_not_found() {
        let r = PluginRegistry::new();
        assert!(r.validate("x", "0.2", PluginCategory::Tool, false).is_err());
    }
    #[test]
    fn plugin_metadata_conservative() {
        let m = PluginMetadata::conservative();
        assert_eq!(
            m.determinism_contract,
            DeterminismContract::NonDeterministic
        );
    }
    #[test]
    fn plugin_metadata_pure() {
        let m = PluginMetadata::pure();
        assert_eq!(m.determinism_contract, DeterminismContract::Deterministic);
    }
    #[test]
    fn allow_in_replay_uses_replay_safe() {
        assert!(allow_in_replay(&PluginMetadata::pure()));
    }
    #[test]
    fn requires_sandbox_uses_side_effects() {
        assert!(requires_sandbox(&PluginMetadata::conservative()));
    }
    #[test]
    fn plugin_execution_mode_as_str() {
        assert_eq!(PluginExecutionMode::InProcess.as_str(), "in_process");
    }
    #[test]
    fn route_to_execution_mode_pure_in_process() {
        assert_eq!(
            route_to_execution_mode(&PluginMetadata::pure()),
            PluginExecutionMode::InProcess
        );
    }
    #[test]
    fn validate_plugin_compatibility_empty_ok() {
        assert!(validate_plugin_compatibility(&PluginCompatibility::default(), "0.2.7").is_ok());
    }
    #[test]
    fn validate_plugin_compatibility_gte() {
        let c = PluginCompatibility {
            kernel_compat: ">=0.2.0".into(),
            ..Default::default()
        };
        assert!(validate_plugin_compatibility(&c, "0.2.7").is_ok());
    }
}
