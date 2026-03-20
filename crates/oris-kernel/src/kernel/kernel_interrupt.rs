//! Kernel Interrupt Object: kernel-level interrupt with checkpoint and state capture (K3-a).

use crate::kernel::identity::RunId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type KernelInterruptId = String;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KernelInterruptKind {
    HumanInTheLoop,
    ApprovalRequired,
    ToolCallWaiting,
    Checkpoint,
    ResourceExhausted,
    ErrorRecovery,
    Custom(String),
}

impl KernelInterruptKind {
    pub fn requires_input(&self) -> bool {
        matches!(
            self,
            KernelInterruptKind::HumanInTheLoop
                | KernelInterruptKind::ApprovalRequired
                | KernelInterruptKind::ToolCallWaiting
                | KernelInterruptKind::ErrorRecovery
        )
    }
    pub fn is_checkpoint(&self) -> bool {
        matches!(self, KernelInterruptKind::Checkpoint)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KernelInterruptStatus {
    Pending,
    Resolving,
    Resolved,
    Rejected,
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterruptCheckpoint {
    pub seq: crate::kernel::Seq,
    pub state_snapshot: serde_json::Value,
    pub state_hash: String,
    pub step_id: Option<String>,
    pub checkpointed_at: DateTime<Utc>,
}

impl InterruptCheckpoint {
    pub fn new(
        seq: crate::kernel::Seq,
        state_snapshot: serde_json::Value,
        state_hash: String,
        step_id: Option<String>,
    ) -> Self {
        Self {
            seq,
            state_snapshot,
            state_hash,
            step_id,
            checkpointed_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelInterrupt {
    pub id: KernelInterruptId,
    pub thread_id: RunId,
    pub kind: KernelInterruptKind,
    pub status: KernelInterruptStatus,
    pub payload_schema: serde_json::Value,
    pub checkpoint: Option<InterruptCheckpoint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub step_id: Option<String>,
    pub reason: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl KernelInterrupt {
    pub fn new(
        id: KernelInterruptId,
        thread_id: RunId,
        kind: KernelInterruptKind,
        payload_schema: serde_json::Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            thread_id,
            kind,
            status: KernelInterruptStatus::Pending,
            payload_schema,
            checkpoint: None,
            created_at: now,
            updated_at: now,
            step_id: None,
            reason: None,
            metadata: None,
        }
    }
    pub fn with_step(mut self, step_id: String) -> Self {
        self.step_id = Some(step_id);
        self.updated_at = Utc::now();
        self
    }
    pub fn with_reason(mut self, reason: String) -> Self {
        self.reason = Some(reason);
        self.updated_at = Utc::now();
        self
    }
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self.updated_at = Utc::now();
        self
    }
    pub fn with_checkpoint(mut self, checkpoint: InterruptCheckpoint) -> Self {
        self.checkpoint = Some(checkpoint);
        self.updated_at = Utc::now();
        self
    }
    pub fn start_resolving(&mut self) -> Result<(), KernelInterruptError> {
        if self.status != KernelInterruptStatus::Pending {
            return Err(KernelInterruptError::InvalidStatusTransition {
                from: self.status.clone(),
                to: "Resolving".into(),
            });
        }
        self.status = KernelInterruptStatus::Resolving;
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn resolve(&mut self) -> Result<(), KernelInterruptError> {
        if self.status != KernelInterruptStatus::Resolving {
            return Err(KernelInterruptError::InvalidStatusTransition {
                from: self.status.clone(),
                to: "Resolved".into(),
            });
        }
        self.status = KernelInterruptStatus::Resolved;
        self.updated_at = Utc::now();
        Ok(())
    }
    pub fn is_pending(&self) -> bool {
        self.status == KernelInterruptStatus::Pending
    }
    pub fn is_resolved(&self) -> bool {
        self.status == KernelInterruptStatus::Resolved
    }
    pub fn can_resume(&self) -> bool {
        self.checkpoint.is_some() && self.is_pending()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KernelInterruptError {
    #[error("Kernel interrupt store error: {0}")]
    Store(String),
    #[error("Kernel interrupt not found: {0}")]
    NotFound(KernelInterruptId),
    #[error("Invalid status transition from {from:?} to {to}")]
    InvalidStatusTransition {
        from: KernelInterruptStatus,
        to: String,
    },
}

pub trait KernelInterruptStore: Send + Sync {
    fn save(&self, interrupt: &KernelInterrupt) -> Result<(), KernelInterruptError>;
    fn load(&self, id: &KernelInterruptId)
        -> Result<Option<KernelInterrupt>, KernelInterruptError>;
    fn load_for_run(&self, thread_id: &RunId)
        -> Result<Vec<KernelInterrupt>, KernelInterruptError>;
    fn delete(&self, id: &KernelInterruptId) -> Result<(), KernelInterruptError>;
}

#[derive(Debug, Default)]
pub struct InMemoryKernelInterruptStore {
    by_id: std::sync::RwLock<std::collections::HashMap<KernelInterruptId, KernelInterrupt>>,
}
impl InMemoryKernelInterruptStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KernelInterruptStore for InMemoryKernelInterruptStore {
    fn save(&self, interrupt: &KernelInterrupt) -> Result<(), KernelInterruptError> {
        let mut g = self
            .by_id
            .write()
            .map_err(|e| KernelInterruptError::Store(e.to_string()))?;
        g.insert(interrupt.id.clone(), interrupt.clone());
        Ok(())
    }
    fn load(
        &self,
        id: &KernelInterruptId,
    ) -> Result<Option<KernelInterrupt>, KernelInterruptError> {
        let g = self
            .by_id
            .read()
            .map_err(|e| KernelInterruptError::Store(e.to_string()))?;
        Ok(g.get(id).cloned())
    }
    fn load_for_run(
        &self,
        thread_id: &RunId,
    ) -> Result<Vec<KernelInterrupt>, KernelInterruptError> {
        let g = self
            .by_id
            .read()
            .map_err(|e| KernelInterruptError::Store(e.to_string()))?;
        Ok(g.values()
            .filter(|i| i.thread_id == *thread_id)
            .cloned()
            .collect())
    }
    fn delete(&self, id: &KernelInterruptId) -> Result<(), KernelInterruptError> {
        let mut g = self
            .by_id
            .write()
            .map_err(|e| KernelInterruptError::Store(e.to_string()))?;
        g.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_kernel_interrupt() {
        let ki = KernelInterrupt::new(
            "ki-1".into(),
            "run-1".into(),
            KernelInterruptKind::HumanInTheLoop,
            serde_json::json!({"type": "string"}),
        );
        assert_eq!(ki.id, "ki-1");
        assert!(ki.is_pending());
        assert!(!ki.is_resolved());
    }
    #[test]
    fn kernel_interrupt_with_checkpoint() {
        let checkpoint = InterruptCheckpoint::new(
            10,
            serde_json::json!({"counter": 5}),
            "abc123".into(),
            Some("step-1".into()),
        );
        let ki = KernelInterrupt::new(
            "ki-2".into(),
            "run-1".into(),
            KernelInterruptKind::Checkpoint,
            serde_json::json!({}),
        )
        .with_checkpoint(checkpoint);
        assert!(ki.can_resume());
    }
    #[test]
    fn interrupt_requires_input() {
        assert!(KernelInterruptKind::HumanInTheLoop.requires_input());
        assert!(KernelInterruptKind::ApprovalRequired.requires_input());
        assert!(!KernelInterruptKind::Checkpoint.requires_input());
    }
    #[test]
    fn kernel_interrupt_status_transition() {
        let mut ki = KernelInterrupt::new(
            "ki-3".into(),
            "run-1".into(),
            KernelInterruptKind::ToolCallWaiting,
            serde_json::json!({}),
        );
        ki.start_resolving().unwrap();
        assert_eq!(ki.status, KernelInterruptStatus::Resolving);
        ki.resolve().unwrap();
        assert!(ki.is_resolved());
    }
}
