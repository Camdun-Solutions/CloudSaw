// Filesystem write surface for the report exporter (Contract 15
// §Constraints).
//
// `write_export` is the single function that produces a report file
// on disk:
//
//   * The output path comes from the IPC argument, which the
//     frontend MUST source from the native save dialog. Any path
//     ending with a directory separator is rejected (defense in
//     depth against an empty filename).
//   * The file is written atomically: bytes go to a sibling
//     `*.partial` file first, then renamed into place. A read-only
//     destination causes the partial file to fail to create and the
//     primary path is never created with partial content.
//   * Permissions are narrowed to user-only via the existing
//     `db::paths::set_user_only` helper.
//   * Auto-export, when enabled, copies the just-written file to the
//     configured folder. Failures there NEVER fail the primary
//     export; the IPC returns the success outcome with a
//     `auto_export_failed` flag set.
//   * An event-log row is written for every successful primary
//     export. The row's `path` field carries the chosen output path;
//     no scan content or finding text is mirrored into the log.

use std::path::{Path, PathBuf};

use super::error::ReportsError;
use super::model::{ExportOutcome, ReportContent, ReportKind};
use super::settings;
use crate::db::paths::set_user_only;
use crate::eventlog::{record_event, EventInput, EventKind};

pub fn write_export(
    output_path: &str,
    bytes: &[u8],
    content: &ReportContent,
    format_label: &str,
) -> Result<ExportOutcome, ReportsError> {
    if output_path.is_empty() || output_path.ends_with('/') || output_path.ends_with('\\') {
        return Err(ReportsError::InvalidInput("output_path"));
    }
    let target = PathBuf::from(output_path);
    let parent = target
        .parent()
        .ok_or(ReportsError::InvalidInput("output_path"))?;
    if !parent.as_os_str().is_empty() && !parent.is_dir() {
        return Err(ReportsError::OutputWrite);
    }

    // Atomic write: write to a `.partial` sibling, then rename.
    let mut partial = target.clone();
    let partial_name = format!(
        "{}.partial",
        target
            .file_name()
            .ok_or(ReportsError::InvalidInput("output_path"))?
            .to_string_lossy()
    );
    partial.set_file_name(partial_name);

    std::fs::write(&partial, bytes).map_err(|_| ReportsError::OutputWrite)?;
    // Narrow permissions BEFORE rename so the in-place file lands with
    // the right ACL on Unix.
    set_user_only(&partial, false).map_err(|e| ReportsError::Io(e.to_string()))?;
    std::fs::rename(&partial, &target).map_err(|_| {
        // Best-effort cleanup of the partial.
        let _ = std::fs::remove_file(&partial);
        ReportsError::OutputWrite
    })?;
    set_user_only(&target, false).map_err(|e| ReportsError::Io(e.to_string()))?;

    let bytes_written = bytes.len() as u64;

    // Auto-export copy.
    let s = settings::read().unwrap_or(settings::ReportSettings {
        auto_export_enabled: false,
        auto_export_folder: None,
        mask_account_ids_default: true,
    });
    let mut auto_export_path: Option<String> = None;
    let mut auto_export_failed = false;
    if s.auto_export_enabled {
        if let Some(folder) = s.auto_export_folder.as_deref() {
            match try_auto_export(&target, folder) {
                Ok(p) => auto_export_path = Some(p.to_string_lossy().to_string()),
                Err(_) => {
                    auto_export_failed = true;
                }
            }
        } else {
            // Toggle on but no folder set — treat as a failed copy so
            // the UI can surface the diagnostic.
            auto_export_failed = true;
        }
    }

    // Event log entry — Contract 15 §Constraints.
    let scan_hint = match content.header.kind {
        ReportKind::PerScan => content.scans.first().map(|s| s.scan_id.clone()),
        ReportKind::Custom => None,
    };
    let mut event = EventInput::new(
        EventKind::Export,
        format!(
            "Report exported ({}, {} findings, {}).",
            format_label,
            content.findings.len(),
            match content.header.disclosure {
                super::model::AccountIdDisclosure::Masked => "masked",
                super::model::AccountIdDisclosure::Full => "full IDs",
            }
        ),
    )
    .with_path(target.to_string_lossy().to_string())
    .with_item_count(content.findings.len() as i64);
    if let Some(sid) = scan_hint {
        event = event.with_scan_id(sid);
    }
    record_event(event);

    Ok(ExportOutcome {
        primary_path: target.to_string_lossy().to_string(),
        bytes_written,
        auto_export_path,
        auto_export_failed,
    })
}

fn try_auto_export(target: &Path, folder: &str) -> Result<PathBuf, std::io::Error> {
    let dir = Path::new(folder);
    if !dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "auto-export folder missing",
        ));
    }
    let name = target.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no filename")
    })?;
    let dest = dir.join(name);
    std::fs::copy(target, &dest)?;
    let _ = set_user_only(&dest, false);
    Ok(dest)
}
