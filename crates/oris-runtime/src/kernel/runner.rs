//! KernelRunner: recommended way to run the kernel from sync or async code.
//!
//! Encapsulates the correct Tokio runtime usage so callers do not need to
//! use `rt.enter()`, `block_in_place`, or `spawn_blocking` manually.
//! Use this instead of calling `kernel.run_until_blocked` directly when
//! using GraphStepFnAdapter or other step functions that require a runtime.

use std::sync::Arc;

use crate::kernel::driver::{Kernel, RunStatus, Signal};
use crate::kernel::identity::RunId;
use crate::kernel::state::KernelState;
use crate::kernel::KernelError;

/// Runner that executes the kernel with correct runtime handling.
///
/// - **Sync**: Runs the kernel on a dedicated thread with its own Tokio runtime,
///   so you can call from any thread without an existing runtime.
/// - **Async**: Uses `spawn_blocking` so the kernel runs on a blocking thread
///   and does not block the async reactor.
pub struct KernelRunner<S: KernelState> {
    kernel: Arc<Kernel<S>>,
}

impl<S: KernelState> KernelRunner<S> {
    /// Creates a runner that will use the given kernel for all runs.
    pub fn new(kernel: Kernel<S>) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    /// Sync entry: runs until blocked/completed on a dedicated thread with an
    /// internal runtime. Blocks the current thread until the run finishes.
    pub fn run_until_blocked_sync(
        &self,
        run_id: &RunId,
        initial_state: S,
    ) -> Result<RunStatus, KernelError> {
        let kernel = Arc::clone(&self.kernel);
        let run_id = run_id.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(KernelError::Driver(e.to_string())));
                    return;
                }
            };
            // Enter the runtime so step adapters' block_on work; do not nest block_on here.
            let _guard = rt.enter();
            let result = kernel.run_until_blocked(&run_id, initial_state);
            let _ = tx.send(result);
        });
        rx.recv()
            .map_err(|_| KernelError::Driver("runner thread panicked or dropped".into()))?
    }

    /// Async entry: runs the kernel inside `spawn_blocking` so the async reactor
    /// is not blocked. Use from async code without deadlock.
    pub async fn run_until_blocked_async(
        &self,
        run_id: &RunId,
        initial_state: S,
    ) -> Result<RunStatus, KernelError> {
        let kernel = Arc::clone(&self.kernel);
        let run_id = run_id.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| KernelError::Driver(e.to_string()))?;
            let _guard = rt.enter();
            kernel.run_until_blocked(&run_id, initial_state)
        })
        .await
        .map_err(|e| KernelError::Driver(e.to_string()))?
    }

    /// Sync resume: same as run_until_blocked_sync but after appending a resume event.
    pub fn resume_sync(
        &self,
        run_id: &RunId,
        initial_state: S,
        signal: Signal,
    ) -> Result<RunStatus, KernelError> {
        let kernel = Arc::clone(&self.kernel);
        let run_id = run_id.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(KernelError::Driver(e.to_string())));
                    return;
                }
            };
            let _guard = rt.enter();
            let result = kernel.resume(&run_id, initial_state, signal);
            let _ = tx.send(result);
        });
        rx.recv()
            .map_err(|_| KernelError::Driver("runner thread panicked or dropped".into()))?
    }

    /// Async resume: same as run_until_blocked_async but after appending a resume event.
    pub async fn resume_async(
        &self,
        run_id: &RunId,
        initial_state: S,
        signal: Signal,
    ) -> Result<RunStatus, KernelError> {
        let kernel = Arc::clone(&self.kernel);
        let run_id = run_id.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| KernelError::Driver(e.to_string()))?;
            let _guard = rt.enter();
            kernel.resume(&run_id, initial_state, signal)
        })
        .await
        .map_err(|e| KernelError::Driver(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::driver::{Kernel, RunStatus};
    use crate::kernel::event_store::InMemoryEventStore;
    use crate::kernel::reducer::StateUpdatedOnlyReducer;
    use crate::kernel::state::KernelState;
    use crate::kernel::stubs::{AllowAllPolicy, NoopActionExecutor, NoopStepFn};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct TestState(u32);
    impl KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

    #[test]
    fn run_until_blocked_sync_completes() {
        let kernel = Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
            effect_sink: None,
            mode: crate::kernel::KernelMode::Normal,
        };
        let runner = KernelRunner::new(kernel);
        let run_id = "runner-sync-test".to_string();
        let status = runner
            .run_until_blocked_sync(&run_id, TestState(0))
            .unwrap();
        assert!(matches!(status, RunStatus::Completed));
    }

    #[tokio::test]
    async fn run_until_blocked_async_completes_no_deadlock() {
        let kernel = Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
            effect_sink: None,
            mode: crate::kernel::KernelMode::Normal,
        };
        let runner = KernelRunner::new(kernel);
        let run_id = "runner-async-test".to_string();
        let status = runner
            .run_until_blocked_async(&run_id, TestState(0))
            .await
            .unwrap();
        assert!(matches!(status, RunStatus::Completed));
    }

    #[tokio::test]
    async fn run_until_blocked_async_twice_no_hang() {
        let kernel = Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
            effect_sink: None,
            mode: crate::kernel::KernelMode::Normal,
        };
        let runner = KernelRunner::new(kernel);
        let status1 = runner
            .run_until_blocked_async(&"run-1".to_string(), TestState(0))
            .await
            .unwrap();
        let status2 = runner
            .run_until_blocked_async(&"run-2".to_string(), TestState(0))
            .await
            .unwrap();
        assert!(matches!(status1, RunStatus::Completed));
        assert!(matches!(status2, RunStatus::Completed));
    }

    /// CI-style: from async context, runner must complete within a timeout (no reactor blocking).
    #[tokio::test]
    async fn run_until_blocked_async_completes_within_timeout() {
        let kernel = Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
            effect_sink: None,
            mode: crate::kernel::KernelMode::Normal,
        };
        let runner = KernelRunner::new(kernel);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            runner.run_until_blocked_async(&"timeout-test".to_string(), TestState(0)),
        )
        .await;
        assert!(
            result.is_ok(),
            "run_until_blocked_async should complete within 5s (no deadlock)"
        );
        let status = result.unwrap().unwrap();
        assert!(matches!(status, RunStatus::Completed));
    }
}
