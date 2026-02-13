//! In-memory EventStore implementation for the kernel.
//!
//! Append is atomic (all or nothing); scan returns events in ascending seq order.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::kernel::event::{Event, EventStore, KernelError, SequencedEvent};
use crate::kernel::identity::{RunId, Seq};

/// In-memory event store: one log per run, seq assigned on append.
pub struct InMemoryEventStore {
    /// run_id -> ordered events (seq 1, 2, 3, ...)
    logs: RwLock<HashMap<RunId, Vec<SequencedEvent>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            logs: RwLock::new(HashMap::new()),
        }
    }

    fn next_seq(log: &[SequencedEvent]) -> Seq {
        log.last().map(|e| e.seq + 1).unwrap_or(1)
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    fn append(&self, run_id: &RunId, events: &[Event]) -> Result<Seq, KernelError> {
        if events.is_empty() {
            let logs = self
                .logs
                .read()
                .map_err(|e| KernelError::EventStore(e.to_string()))?;
            let last = logs
                .get(run_id)
                .and_then(|l| l.last())
                .map(|e| e.seq)
                .unwrap_or(0);
            return Ok(last);
        }
        let mut logs = self
            .logs
            .write()
            .map_err(|e| KernelError::EventStore(e.to_string()))?;
        let log = logs.entry(run_id.clone()).or_default();
        let start_seq = Self::next_seq(log);
        for (i, event) in events.iter().cloned().enumerate() {
            log.push(SequencedEvent {
                seq: start_seq + i as Seq,
                event,
            });
        }
        Ok(*log.last().map(|e| &e.seq).unwrap())
    }

    fn scan(&self, run_id: &RunId, from: Seq) -> Result<Vec<SequencedEvent>, KernelError> {
        let logs = self
            .logs
            .read()
            .map_err(|e| KernelError::EventStore(e.to_string()))?;
        let log = match logs.get(run_id) {
            Some(l) => l,
            None => return Ok(Vec::new()),
        };
        Ok(log.iter().filter(|e| e.seq >= from).cloned().collect())
    }

    fn head(&self, run_id: &RunId) -> Result<Seq, KernelError> {
        let logs = self
            .logs
            .read()
            .map_err(|e| KernelError::EventStore(e.to_string()))?;
        Ok(logs
            .get(run_id)
            .and_then(|l| l.last())
            .map(|e| e.seq)
            .unwrap_or(0))
    }
}

/// Shared event store: wraps `Arc<InMemoryEventStore>` so graph and Kernel can share the same log.
pub struct SharedEventStore(pub std::sync::Arc<InMemoryEventStore>);

impl SharedEventStore {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(InMemoryEventStore::new()))
    }
}

impl Default for SharedEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for SharedEventStore {
    fn append(&self, run_id: &RunId, events: &[Event]) -> Result<Seq, KernelError> {
        self.0.append(run_id, events)
    }

    fn scan(&self, run_id: &RunId, from: Seq) -> Result<Vec<SequencedEvent>, KernelError> {
        self.0.scan(run_id, from)
    }

    fn head(&self, run_id: &RunId) -> Result<Seq, KernelError> {
        self.0.head(run_id)
    }
}
