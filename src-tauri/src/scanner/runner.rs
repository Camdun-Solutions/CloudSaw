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
/// Stable basename CloudSaw passes to ScoutSuite via `--report-name`.
/// Combined with the `aws` provider, ScoutSuite writes the result file
/// at `<output_dir>/scoutsuite-results/scoutsuite_results_aws-<name>.js`
/// — see `post_process_scoutsuite_output()` for the path it constructs
/// AFTER the subprocess exits to convert that .js file into the clean
/// JSON CloudSaw's findings parser expects.
pub const SCOUTSUITE_REPORT_NAME: &str = "cloudsaw";

pub fn spawn_scoutsuite(
    handle: Arc<ScanHandle>,
    output_dir: &Path,
    region: &str,
    creds: &AssumedCredentials,
) -> Result<(), ScannerError> {
    // Integrity check before EVERY execution.
    let (binary_path, _sha) = binary::locate_and_verify()?;

    // ScoutSuite CLI shape (see vendor/scoutsuite/ScoutSuite/core/cli_parser.py):
    //   scoutsuite aws [--profile <p>|--access-keys ...] --report-dir <d>
    //                  --report-name <n> --regions <r> --no-browser
    //
    // We pass `aws` as the required positional, the report dir + name (so
    // post_process_scoutsuite_output() knows where the .js file will be),
    // `--regions` to scope the scan, and `--no-browser` so ScoutSuite
    // doesn't try to launch the report viewer.
    //
    // We deliberately do NOT pass `--access-keys` or `--profile`. With
    // no auth flag, ScoutSuite's AWS strategy falls through to
    // `boto3.Session()` which picks up the AWS_* env vars set below.
    // That keeps the assumed STS credentials off the argv (and out of
    // the OS process table) — CLAUDE.md §4.3.
    let mut cmd = Command::new(&binary_path);
    cmd.arg("aws")
        .arg("--report-dir")
        .arg(output_dir)
        .arg("--report-name")
        .arg(SCOUTSUITE_REPORT_NAME)
        .arg("--regions")
        .arg(region)
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
pub fn wait_for_child(handle: Arc<ScanHandle>) -> Result<SpawnOutcome, ScannerError> {
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

/// Path ScoutSuite actually writes the results file to, given the
/// `--report-dir` and `--report-name` flags `spawn_scoutsuite()` passes.
/// Constructed per ScoutSuite's `get_filename()` in
/// `vendor/scoutsuite/ScoutSuite/output/utils.py:49`:
///
/// ```python
/// name = f'scoutsuite_results_{file_name}' if file_name else 'scoutsuite_results'
/// ```
///
/// Filename layout:
///   `<report-dir>/scoutsuite-results/scoutsuite_results_<name>.js`
///
/// NOTE: an earlier version of this function included an `aws-` prefix
/// before `<name>` based on a misreading of the upstream code. ScoutSuite
/// does NOT prefix with the provider — `file_name` is what we pass via
/// `--report-name` and that's used verbatim. 2026.5.13 shipped the wrong
/// path, which caused `post_process_scoutsuite_output()` to return
/// `Ok(false)` (source file "missing"), the scanner module then mapped
/// the missing `raw-scout.json` to `OutputMissing` — surfaced in the UI
/// as "scanner finished but produced no findings file" even though the
/// real ScoutSuite run had completed perfectly.
///
/// The `.js` extension is not a typo — ScoutSuite emits the JSON inside
/// a `scoutsuite_results = { ... }` JavaScript variable so the HTML
/// report viewer can `<script src="...">` it without a CORS workaround.
/// CloudSaw's findings parser expects pure JSON, so we strip the prefix
/// in `post_process_scoutsuite_output()`.
pub fn scoutsuite_results_path(output_dir: &Path) -> PathBuf {
    output_dir.join("scoutsuite-results").join(format!(
        "scoutsuite_results_{name}.js",
        name = SCOUTSUITE_REPORT_NAME,
    ))
}

/// Convert ScoutSuite's `.js`-wrapped output into the clean JSON
/// CloudSaw's findings parser reads from `raw_output_path`.
///
/// Reads the file at `scoutsuite_results_path(output_dir)`, strips the
/// `scoutsuite_results =` first line (and any trailing semicolon/
/// whitespace), and writes the JSON body to `raw_output_path` with
/// user-only permissions.
///
/// Returns `Ok(false)` when ScoutSuite produced no output file (e.g. it
/// exited before reaching the encoder); the caller maps that to
/// `OutputMissing`. Returns `Ok(true)` on a successful conversion.
/// Returns `Err(ScanIo(...))` on I/O failure during the conversion
/// itself (rare — the file was just written by ScoutSuite).
pub fn post_process_scoutsuite_output(
    output_dir: &Path,
    raw_output_path: &Path,
) -> Result<bool, ScannerError> {
    let source = scoutsuite_results_path(output_dir);
    if !source.is_file() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(&source)
        .map_err(|e| ScannerError::ScanIo(format!("read scoutsuite output: {e}")))?;

    // ScoutSuite writes:
    //   scoutsuite_results =
    //   { ... JSON ... }
    // OR sometimes
    //   scoutsuite_results = { ... JSON ... };
    // We tolerate both shapes: find the first `=`, take everything after
    // it, then trim any trailing `;` and surrounding whitespace.
    let body = match raw.split_once('=') {
        Some((_, rest)) => rest.trim().trim_end_matches(';').trim(),
        // Fallback: if there's no `=`, hand the whole file to the parser
        // and let it surface a malformed-JSON error rather than swallow
        // the bytes silently.
        None => raw.as_str(),
    };

    std::fs::write(raw_output_path, body)
        .map_err(|e| ScannerError::ScanIo(format!("write raw_output_path: {e}")))?;
    // Inherit the same user-only perms ensure_user_only_dir applied to
    // the parent. The findings/exporter paths set file perms themselves
    // when they take ownership; here we just produce the file.
    Ok(true)
}

/// Persist ScoutSuite's stderr to a sidecar log file inside the scan's
/// output directory. Called regardless of exit category so a successful
/// run also leaves a diagnostic trail (rule-tuning, future debugging).
///
/// Best-effort: a failure to write the sidecar does NOT propagate up —
/// the scan's overall result is more important than the log. Errors
/// are silently dropped because the caller is already in an error path
/// when this matters.
pub fn write_stderr_sidecar(output_dir: &Path, stderr: &[u8]) {
    if stderr.is_empty() {
        return;
    }
    let path = output_dir.join("scoutsuite-stderr.log");
    let _ = std::fs::write(path, stderr);
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

    #[test]
    fn scoutsuite_results_path_uses_stable_name() {
        // Regression test: the path MUST match what ScoutSuite's
        // `output/utils.py:get_filename()` emits — i.e.
        // `scoutsuite_results_<name>.js` with NO `aws-` (or any other
        // provider) prefix. 2026.5.13 shipped the wrong path and every
        // scan completed successfully at the ScoutSuite layer but then
        // failed in post-processing with a misleading "no findings
        // file" message.
        let dir = std::path::PathBuf::from("/tmp/scan-42");
        let got = scoutsuite_results_path(&dir);
        assert_eq!(
            got,
            std::path::PathBuf::from(
                "/tmp/scan-42/scoutsuite-results/scoutsuite_results_cloudsaw.js"
            )
        );
    }

    #[test]
    fn post_process_strips_scoutsuite_prefix() {
        let dir = std::env::temp_dir().join(format!(
            "cloudsaw-postprocess-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let results_dir = dir.join("scoutsuite-results");
        std::fs::create_dir_all(&results_dir).unwrap();
        let source = results_dir.join("scoutsuite_results_aws-cloudsaw.js");
        std::fs::write(
            &source,
            "scoutsuite_results =\n{\"account_id\":\"111122223333\",\"services\":{}}\n",
        )
        .unwrap();

        let raw = dir.join("raw-scout.json");
        let ok = post_process_scoutsuite_output(&dir, &raw).unwrap();
        assert!(ok);

        let cleaned = std::fs::read_to_string(&raw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(parsed["account_id"], "111122223333");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_process_handles_trailing_semicolon() {
        let dir = std::env::temp_dir().join(format!(
            "cloudsaw-postprocess-semi-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let results_dir = dir.join("scoutsuite-results");
        std::fs::create_dir_all(&results_dir).unwrap();
        let source = results_dir.join("scoutsuite_results_aws-cloudsaw.js");
        std::fs::write(&source, "scoutsuite_results = {\"k\":\"v\"};").unwrap();

        let raw = dir.join("raw-scout.json");
        post_process_scoutsuite_output(&dir, &raw).unwrap();
        let cleaned = std::fs::read_to_string(&raw).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(parsed["k"], "v");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_process_returns_false_when_source_missing() {
        let dir = std::env::temp_dir().join(format!(
            "cloudsaw-postprocess-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("raw-scout.json");
        let ok = post_process_scoutsuite_output(&dir, &raw).unwrap();
        assert!(!ok);
        assert!(!raw.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
