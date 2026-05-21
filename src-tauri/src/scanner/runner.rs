// Spawn-and-wait core for the scanner orchestrator.
//
// CLAUDE.md §4.5 + Contract 06 §Constraints:
//   * External binaries are invoked by ABSOLUTE PATH with ARGV ARRAYS. A
//     shell is NEVER used.
//   * The binary's SHA-256 is verified against the build-pinned hash BEFORE
//     every execution.
//   * Temporary credentials reach the child via its environment only — not
//     disk, not logs, not the parent process environment.
//   * stdout/stderr capture is bounded; the truncated flag is propagated.
//
// We use `std::process::Command` directly. `Command::new(path).arg(...)`
// never invokes a shell — there is no /bin/sh layer, no PATH lookup, no
// string interpolation. Every argument is a separate argv entry. The
// integrity check happens immediately before spawn.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use super::binary;
use super::error::ScannerError;
use super::handles::ScanHandle;
use super::sts::AssumedCredentials;

/// Cap stdout/stderr at this many bytes per stream. ScoutSuite chatters a
/// lot on large accounts; the contract calls for bounded capture with a
/// `truncated` flag rather than unbounded growth. The raw findings file is
/// written directly by the scanner and is not subject to this cap.
const STREAM_CAP_BYTES: usize = 4 * 1024 * 1024;

/// Polling interval for the wait loop. Short enough that cancellation
/// terminates the process promptly; long enough that idle CPU stays low.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// One ScoutSuite child's outcome from the wait loop.
pub struct SpawnOutcome {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub canceled: bool,
}

/// Locate-and-verify the bundled scanner binary, then spawn it with the
/// supplied argv and per-child environment. `creds` is consumed by reference
/// and copied into the child's env via `Command::env`; the source buffer is
/// owned by the caller and dropped after this returns.
///
/// The function does NOT poll the child — it returns the spawned Child via
/// the supplied handle. Use `wait_for_child` to await the exit status.
pub fn spawn_scoutsuite(
    handle: Arc<ScanHandle>,
    output_dir: &Path,
    raw_output_path: &Path,
    aws_account_id: &str,
    region: &str,
    creds: &AssumedCredentials,
) -> Result<(), ScannerError> {
    // Integrity check before EVERY execution.
    let (binary_path, _sha) = binary::locate_and_verify()?;

    let mut cmd = Command::new(&binary_path);
    cmd.arg("--account-id")
        .arg(aws_account_id)
        .arg("--report-dir")
        .arg(output_dir)
        .arg("--output")
        .arg(raw_output_path)
        .arg("--no-browser");
    cmd.current_dir(output_dir);

    // Credentials live ONLY on the child's environment. `Command::env` does
    // NOT modify the parent process — it adds the value to a per-Command
    // map that the OS hands off to the child at spawn time.
    cmd.env("AWS_ACCESS_KEY_ID", &creds.access_key_id);
    cmd.env("AWS_SECRET_ACCESS_KEY", &creds.secret_access_key);
    cmd.env("AWS_SESSION_TOKEN", &creds.session_token);
    cmd.env("AWS_DEFAULT_REGION", region);
    // ScoutSuite respects this; setting it explicitly avoids the binary
    // sniffing the parent's region.
    cmd.env("AWS_REGION", region);
    // Disable any version-check phone-home behavior the bundled binary
    // might still carry; CloudSaw never sends scan data off-host.
    cmd.env("SCOUTSUITE_TELEMETRY", "0");

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|_| ScannerError::SpawnFailed)?;
    handle.attach_child(child);
    Ok(())
}

/// Wait for the attached child to exit, polling the cancel flag between
/// `try_wait` iterations. Returns the captured streams and the exit code.
///
/// Cancellation: when `handle.is_canceled()` is true, the child is killed
/// (the cancel path may already have done so) and we still wait for the
/// process to be reaped — Linux/macOS leak a zombie if we don't.
pub fn wait_for_child(
    handle: Arc<ScanHandle>,
) -> Result<SpawnOutcome, ScannerError> {
    let mut child = handle
        .take_child()
        .ok_or(ScannerError::Internal("handle_missing_child"))?;

    // Drain stdout/stderr in background threads with a hard cap. We can't
    // safely interleave `try_wait` with a blocking `read_to_end` on stdout
    // — the child might wedge on a full pipe while we wait on its exit.
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let stdout_thread = stdout_handle.map(|s| spawn_bounded_reader(s, STREAM_CAP_BYTES));
    let stderr_thread = stderr_handle.map(|s| spawn_bounded_reader(s, STREAM_CAP_BYTES));

    let exit_status = loop {
        if handle.is_canceled() {
            // The cancel path already called `kill()`; that won't cause
            // `try_wait` to return immediately, so we keep looping until
            // the OS reaps the child.
            let _ = child.kill();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                std::thread::sleep(WAIT_POLL_INTERVAL);
                continue;
            }
            Err(_) => return Err(ScannerError::ProcessLost),
        }
    };

    let (stdout, stdout_truncated) = stdout_thread
        .map(|h| h.join().unwrap_or_else(|_| (Vec::new(), false)))
        .unwrap_or((Vec::new(), false));
    let (stderr, stderr_truncated) = stderr_thread
        .map(|h| h.join().unwrap_or_else(|_| (Vec::new(), false)))
        .unwrap_or((Vec::new(), false));

    Ok(SpawnOutcome {
        exit_code: exit_status.code(),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
        canceled: handle.is_canceled(),
    })
}

/// Read up to `cap` bytes from `reader` into a Vec, returning `(bytes,
/// truncated)`. We don't fail the scan if the stream is unreadable — we
/// just return what we got.
fn spawn_bounded_reader<R: std::io::Read + Send + 'static>(
    mut reader: R,
    cap: usize,
) -> std::thread::JoinHandle<(Vec<u8>, bool)> {
    std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(8 * 1024);
        let mut chunk = [0u8; 8 * 1024];
        let mut truncated = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n > cap {
                        let room = cap.saturating_sub(buf.len());
                        if room > 0 {
                            buf.extend_from_slice(&chunk[..room]);
                        }
                        truncated = true;
                        // Drain the rest of the stream so the child doesn't
                        // block on a full pipe.
                        loop {
                            match reader.read(&mut chunk) {
                                Ok(0) | Err(_) => break,
                                Ok(_) => continue,
                            }
                        }
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
        (buf, truncated)
    })
}

/// Stable categorization of exit codes into the contract's three branches:
///
/// * 0 -> Complete
/// * 2 -> CompleteWithWarnings (ScoutSuite convention: "succeeded with
///   missing-permission warnings")
/// * anything else (and cancellation) -> caller-handled (Failed/Canceled)
///
/// This is a function rather than inline so QA can assert the mapping
/// stays stable.
pub fn classify_exit(code: Option<i32>) -> ExitCategory {
    match code {
        Some(0) => ExitCategory::Success,
        Some(2) => ExitCategory::PartialSuccess,
        Some(_) | None => ExitCategory::Failure,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCategory {
    Success,
    PartialSuccess,
    Failure,
}

/// Resolve the absolute path to a scan's output directory inside the app
/// data root. Per CLAUDE.md §6.7: `scans/{scan-id}/`. The directory is
/// created (with user-only permissions) by the orchestrator before spawn.
pub fn scan_output_dir(scan_id: &str) -> Result<PathBuf, ScannerError> {
    use crate::db::paths::app_data_dir;
    let root = app_data_dir().map_err(|e| ScannerError::ScanIo(e.to_string()))?;
    Ok(root.join("scans").join(scan_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exit_maps_each_branch() {
        assert_eq!(classify_exit(Some(0)), ExitCategory::Success);
        assert_eq!(classify_exit(Some(2)), ExitCategory::PartialSuccess);
        assert_eq!(classify_exit(Some(1)), ExitCategory::Failure);
        assert_eq!(classify_exit(Some(137)), ExitCategory::Failure);
        assert_eq!(classify_exit(None), ExitCategory::Failure);
    }
}
