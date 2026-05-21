// In-memory handles for currently-running scans.
//
// The scanner orchestrator runs each scan on a background thread (a tokio
// task wouldn't help much — `std::process::Child::wait` is blocking). We
// need two things from outside that thread:
//
//   1. `scan_cancel(scan_id)` — terminate the ScoutSuite child mid-run.
//   2. Concurrent-scan check — refuse a second `run_scan(account)` while
//      another scan is in flight for the same account. The SQLite
//      `try_claim_account` transaction is the authoritative gate; this
//      in-memory map is an optimization that avoids hitting SQLite for the
//      cancel path.
//
// Concurrency model: a `Mutex<HashMap<scan_id, Arc<ScanHandle>>>` static.
// Each handle owns an `Arc<Mutex<Option<Child>>>` so the orchestrator can
// `wait()` while the cancel path can `kill()` against the same handle.

use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// One running scan. The orchestrator inserts the handle right before it
/// spawns the ScoutSuite child, then registers the child via
/// `attach_child` once spawned. The cancel path locks the inner mutex and
/// kills the child if present.
pub struct ScanHandle {
    /// Set by the cancel path. The orchestrator's wait loop polls this
    /// between `try_wait` iterations so a kill-then-immediate-exit cleanly
    /// transitions to `Canceled` instead of `Failed`.
    canceled: AtomicBool,
    /// The spawned child. `None` before `attach_child` is called and after
    /// the orchestrator drops the handle (either by reaping the child or
    /// because cancellation already took it).
    child: Mutex<Option<Child>>,
}

impl ScanHandle {
    fn new() -> Self {
        Self {
            canceled: AtomicBool::new(false),
            child: Mutex::new(None),
        }
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }

    /// Attach the spawned child after `Command::spawn()` succeeds. Called
    /// by the orchestrator immediately after spawn.
    pub fn attach_child(&self, child: Child) {
        let mut guard = self.child.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(child);
    }

    /// Reap the child from the handle so the orchestrator can `wait()` on
    /// it. Returns `None` if the child was already taken (e.g. by cancel).
    pub fn take_child(&self) -> Option<Child> {
        let mut guard = self.child.lock().unwrap_or_else(|p| p.into_inner());
        guard.take()
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<ScanHandle>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<ScanHandle>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Insert a fresh handle for `scan_id`. The orchestrator owns the returned
/// Arc; the registry retains a weak claim that the cancel path consults.
pub fn register(scan_id: &str) -> Arc<ScanHandle> {
    let handle = Arc::new(ScanHandle::new());
    let mut guard = registry().lock().unwrap_or_else(|p| p.into_inner());
    guard.insert(scan_id.to_string(), handle.clone());
    handle
}

/// Remove the handle for `scan_id`. Called by the orchestrator on every
/// terminal transition so the registry never carries stale entries.
pub fn unregister(scan_id: &str) {
    let mut guard = registry().lock().unwrap_or_else(|p| p.into_inner());
    guard.remove(scan_id);
}

/// Look up a running scan's handle. Returns `None` once the scan reaches a
/// terminal state (and `unregister` was called).
pub fn lookup(scan_id: &str) -> Option<Arc<ScanHandle>> {
    let guard = registry().lock().unwrap_or_else(|p| p.into_inner());
    guard.get(scan_id).cloned()
}

/// Mark a scan as canceled AND kill the running child if present. Idempotent;
/// the orchestrator's wait loop will notice the cancel flag and transition
/// the scan to `Canceled` even if the child was already dead.
///
/// Returns `false` if no handle exists — the caller (orchestrator IPC) then
/// either reports `ScanNotFound` (if SQLite agrees) or `not running anymore`
/// (if SQLite says the scan is already terminal — already-terminal means
/// cancel is a no-op).
pub fn signal_cancel(scan_id: &str) -> bool {
    let handle = {
        let guard = registry().lock().unwrap_or_else(|p| p.into_inner());
        guard.get(scan_id).cloned()
    };
    let Some(handle) = handle else {
        return false;
    };
    handle.canceled.store(true, Ordering::SeqCst);
    let mut child_guard = handle.child.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(child) = child_guard.as_mut() {
        // best-effort kill; if the child already exited, kill() returns
        // an error we deliberately ignore.
        let _ = child.kill();
    }
    true
}

/// Test seam: clear the registry. The orchestrator process-global state
/// would otherwise leak between integration tests that share the binary.
#[cfg(any(test, debug_assertions))]
pub fn _clear_for_tests() {
    let mut guard = registry().lock().unwrap_or_else(|p| p.into_inner());
    guard.clear();
}
