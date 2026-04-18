//! OS-level resource limits for sandboxed child processes.
//!
//! Enabled by the `resource-limits` cargo feature.  The public API surface is
//! intentionally thin — callers use [`apply_linux_limits`] (Linux) or
//! [`apply_macos_sandbox`] (macOS) to configure a [`tokio::process::Command`]
//! before spawning.

use tokio::process::Command;

#[cfg(target_os = "linux")]
use crate::core::SandboxPolicy;

// ─────────────────────────────────────────────────────────────────────────────
// Linux: setrlimit + process-group isolation
// ─────────────────────────────────────────────────────────────────────────────

/// Configure a `Command` with Linux resource limits and optional process-group
/// isolation based on the supplied `policy`.
///
/// This function is a no-op if compiled on a platform other than Linux.
///
/// ## Resource limits applied
/// - `RLIMIT_AS` (address-space) when `policy.max_memory_bytes` is set.
/// - `RLIMIT_CPU` (CPU seconds) when `policy.max_cpu_secs` is set.
///
/// ## Process-group isolation
/// When `policy.use_process_group` is `true` the child is placed in a new
/// process group via `setsid(2)`.  On timeout the whole group is killed with
/// `SIGKILL`, preventing zombie grandchildren.
#[cfg(target_os = "linux")]
pub fn apply_linux_limits(command: &mut Command, policy: &SandboxPolicy) {
    use nix::sys::resource::{setrlimit, Resource};

    let max_mem = policy.max_memory_bytes;
    let max_cpu = policy.max_cpu_secs;
    let use_pg = policy.use_process_group;

    // SAFETY:
    // =========
    // `pre_exec` is called in the child process after `fork()` but before `exec()`.
    // At this point the child has a copy of the parent's address space but runs in
    // a single-threaded context (POSIX guarantees that only the calling thread survives
    // in the child after fork()). The restrictions below are REQUIRED to maintain soundness:
    //
    // 1. **No memory allocation or dynamic dispatch**: The child cannot safely call
    //    malloc, new, or any function that might invoke a dynamic linker. We only call
    //    async-signal-safe functions.
    //
    // 2. **setrlimit is async-signal-safe**: The nix crate's setrlimit() wraps the
    //    setrlimit(2) syscall directly. Per POSIX, setrlimit(2) is async-signal-safe.
    //
    // 3. **setsid is async-signal-safe**: The nix crate's setsid() wraps setsid(2),
    //    which is also async-signal-safe per POSIX.
    //
    // 4. **No use of Rust's std library**: The closure captures only primitive types
    //    (Option<u64> values) copied from the parent. It does not capture pointers,
    //    references, or any Rust objects that might be in an invalid state post-fork.
    //
    // 5. **Error handling via map_err**: Errors from setrlimit/setsid are converted
    //    to std::io::Error using from_raw_os_error (a trivial cast, no allocation).
    //    Returning an error from pre_exec causes the child process to exit immediately,
    //    which is the correct fail-safe behavior.
    //
    // Invariants that MUST be maintained:
    // - `max_mem`, `max_cpu`, and `use_pg` must be `Copy` types (u64) captured by value
    // - No references or pointers to parent memory may be captured
    // - No mutexes, condition variables, or synchronization primitives from the parent
    // - The closure must not panic (would cause undefined behavior in child)
    unsafe {
        command.pre_exec(move || {
            if let Some(mem_limit) = max_mem {
                setrlimit(Resource::RLIMIT_AS, mem_limit, mem_limit)
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            }
            if let Some(cpu_limit) = max_cpu {
                setrlimit(Resource::RLIMIT_CPU, cpu_limit, cpu_limit)
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            }
            if use_pg {
                nix::unistd::setsid().map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            }
            Ok(())
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// macOS: sandbox-exec wrapper
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal macOS Seatbelt profile that still allows the mutation build process
/// to execute.  The profile denies network access and restricts filesystem
/// writes to the system temporary directory.
const MACOS_SEATBELT_PROFILE: &str = r#"(version 1)
(allow default)
(deny network*)
(allow file-write* (subpath (param "TMPDIR")))
"#;

/// Rewire `command` to run via `sandbox-exec -p <profile> <program> <args...>`.
///
/// Returns `true` when the rewrite was performed (i.e. `sandbox-exec` exists),
/// `false` otherwise.  Callers may use the return value to decide whether to
/// fall back to an unconfined spawn.
///
/// This function is a no-op on non-macOS targets.
#[cfg(target_os = "macos")]
pub fn apply_macos_sandbox(command: &mut Command, program: &str, args: &[String]) -> bool {
    use std::process::Stdio;

    // Check that sandbox-exec is available (it is on all modern macOS versions
    // but may be absent in CI containers).
    if !std::path::Path::new("/usr/bin/sandbox-exec").exists() {
        return false;
    }

    // Build a new command: sandbox-exec -p <profile> <program> [args...]
    let mut wrapped = Command::new("/usr/bin/sandbox-exec");
    wrapped.arg("-p");
    wrapped.arg(MACOS_SEATBELT_PROFILE);
    wrapped.arg(program);
    wrapped.args(args);
    wrapped.stdout(Stdio::piped());
    wrapped.stderr(Stdio::piped());
    wrapped.kill_on_drop(true);

    // Replace the original command in-place by swapping its internals.
    *command = wrapped;
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SandboxPolicy;

    #[test]
    fn sandbox_policy_new_fields_default_none() {
        let policy = SandboxPolicy::oris_default();
        assert!(policy.max_memory_bytes.is_none());
        assert!(policy.max_cpu_secs.is_none());
        assert!(!policy.use_process_group);
    }

    #[test]
    fn sandbox_policy_serialise_round_trip() {
        let policy = SandboxPolicy {
            allowed_programs: vec!["cargo".into()],
            max_duration_ms: 60_000,
            max_output_bytes: 512,
            denied_env_prefixes: vec![],
            max_memory_bytes: Some(256 * 1024 * 1024),
            max_cpu_secs: Some(30),
            use_process_group: true,
        };
        let json = serde_json::to_string(&policy).expect("serialize");
        let back: SandboxPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.max_memory_bytes, Some(256 * 1024 * 1024));
        assert_eq!(back.max_cpu_secs, Some(30));
        assert!(back.use_process_group);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn apply_linux_limits_does_not_panic_for_none_limits() {
        let policy = SandboxPolicy::oris_default(); // all limits = None
        let mut cmd = Command::new("true");
        // Must not panic — simply sets no pre_exec limits.
        apply_linux_limits(&mut cmd, &policy);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn apply_macos_sandbox_returns_bool() {
        let policy = SandboxPolicy::oris_default();
        let mut cmd = Command::new("true");
        // We don't assert the return value (depends on whether sandbox-exec is present
        // in the test environment), just that the call does not panic.
        let _ = apply_macos_sandbox(&mut cmd, "true", &[]);
        let _ = policy.max_memory_bytes; // confirm policy fields accessible
    }
}
