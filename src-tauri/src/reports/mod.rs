// Report exporter — Contract 15.
//
// Two report shapes share one rendering pipeline:
//
//   * 15A. Per-scan report — `export_scan_html` / `export_scan_pdf`.
//     Lists every finding in a single scan with severity breakdown,
//     KB remediation, and compliance mapping.
//   * 15B. Custom report — `export_custom_html` / `export_custom_pdf`.
//     Aggregates over a date range and an explicit account scope:
//     scans, findings (deduplicated), per-service totals, and event-
//     log activity in the range.
//
// Shared invariants (Contract 15 §Constraints):
//
//   * Output paths come from the native save dialog (or the
//     auto-export folder). The IPC accepts a string; the frontend
//     MUST source it from `dialog.save()`. The Rust side rejects
//     empty or directory-shaped paths.
//   * Generated HTML is self-contained: zero `<script>` tags, zero
//     remote URLs, zero external resource loads. Tests assert this
//     on every shape of report.
//   * A mandatory review banner, generation timestamp, and CloudSaw
//     version live in the header of every report.
//   * Account IDs are masked by default; full IDs only on explicit
//     user opt-in. The aggregator pre-renders every account-shaped
//     value to the chosen disclosure mode — the renderers write
//     verbatim.
//   * Files land with user-only permissions; large reports stream
//     through a single allocation cap (`RESOURCE_CAP_PER_FINDING`
//     and `CUSTOM_FINDING_CAP` in `aggregator.rs`).
//   * Every export records an event-log row (Contract 11). The row
//     stores a count + the output path, never the report content.

pub mod aggregator;
pub mod error;
pub mod exporter;
pub mod html;
pub mod model;
pub mod pdf;
pub mod settings;

pub use error::ReportsError;
pub use model::{
    AccountIdDisclosure, ExportOutcome, ReportContent, ReportKind,
};
pub use settings::ReportSettings;

use chrono::{DateTime, Utc};

/// Per-scan HTML export.
pub fn export_scan_html(
    scan_id: &str,
    output_path: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, ReportsError> {
    let content = aggregator::build_per_scan(scan_id, disclosure)?;
    let html = html::render(&content);
    exporter::write_export(output_path, html.as_bytes(), &content, "html")
}

/// Per-scan PDF export.
pub fn export_scan_pdf(
    scan_id: &str,
    output_path: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, ReportsError> {
    let content = aggregator::build_per_scan(scan_id, disclosure)?;
    let bytes = pdf::render(&content)?;
    exporter::write_export(output_path, &bytes, &content, "pdf")
}

/// Custom-range HTML export.
pub fn export_custom_html(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    account_scope: &[String],
    output_path: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, ReportsError> {
    let content = aggregator::build_custom(start, end, account_scope, disclosure)?;
    let html = html::render(&content);
    exporter::write_export(output_path, html.as_bytes(), &content, "html")
}

/// Custom-range PDF export.
pub fn export_custom_pdf(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    account_scope: &[String],
    output_path: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, ReportsError> {
    let content = aggregator::build_custom(start, end, account_scope, disclosure)?;
    let bytes = pdf::render(&content)?;
    exporter::write_export(output_path, &bytes, &content, "pdf")
}

/// Build the report content without writing it. Used by the UI to
/// drive a "preview before save" flow if/when it adopts one.
pub fn preview_scan(
    scan_id: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, ReportsError> {
    aggregator::build_per_scan(scan_id, disclosure)
}

pub fn preview_custom(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    account_scope: &[String],
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, ReportsError> {
    aggregator::build_custom(start, end, account_scope, disclosure)
}

pub fn get_settings() -> Result<ReportSettings, ReportsError> {
    settings::read()
}

pub fn set_settings(s: ReportSettings) -> Result<(), ReportsError> {
    settings::write(&s)
}

/// Default disclosure mode the UI should use when opening the export
/// dialog. Reflects the `report_mask_account_ids_default` setting.
pub fn default_disclosure() -> AccountIdDisclosure {
    let masked = settings::read()
        .map(|s| s.mask_account_ids_default)
        .unwrap_or(true);
    if masked {
        AccountIdDisclosure::Masked
    } else {
        AccountIdDisclosure::Full
    }
}
