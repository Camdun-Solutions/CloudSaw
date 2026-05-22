// Typed error enum for the report exporter. Stable codes only; no raw
// rusqlite text and no filesystem path content escapes through the IPC
// boundary.

use thiserror::Error;

use crate::errors::AppError;
use crate::findings::FindingsError;

#[derive(Debug, Error)]
pub enum ReportsError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// The `scan_id` looked at by `export_scan_*` doesn't exist.
    #[error("scan not found")]
    ScanNotFound,

    /// `findings::*` returned an error while gathering the report's
    /// data. Inner string is the wrapped FindingsError's Display form.
    #[error("findings: {0}")]
    Findings(String),

    /// Output filesystem op failed (read-only path, no parent dir,
    /// permission denied). Distinct from `Render` so the UI can show
    /// the matching "save failed" copy.
    #[error("output write failed")]
    OutputWrite,

    /// PDF render failed. Inner is the printpdf message, already
    /// free of credential / path content (printpdf's own errors are
    /// short status codes).
    #[error("pdf render failed: {0}")]
    PdfRender(String),

    /// User-configurable auto-export folder failed when we tried to
    /// copy the report into it. The in-app export still succeeded —
    /// this is a non-fatal notice the IPC surfaces alongside the
    /// successful primary export.
    #[error("auto-export copy failed")]
    AutoExportCopy,

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl ReportsError {
    pub fn code(&self) -> &'static str {
        match self {
            ReportsError::InvalidInput(_) => "invalid_input",
            ReportsError::ScanNotFound => "scan_not_found",
            ReportsError::Findings(_) => "findings_error",
            ReportsError::OutputWrite => "report_output_write",
            ReportsError::PdfRender(_) => "report_pdf_render",
            ReportsError::AutoExportCopy => "report_auto_export_copy",
            ReportsError::Db(_) => "db_error",
            ReportsError::Io(_) => "io_error",
        }
    }
}

impl From<rusqlite::Error> for ReportsError {
    fn from(e: rusqlite::Error) -> Self {
        ReportsError::Db(e.to_string())
    }
}

impl From<std::io::Error> for ReportsError {
    fn from(e: std::io::Error) -> Self {
        ReportsError::Io(e.to_string())
    }
}

impl From<FindingsError> for ReportsError {
    fn from(e: FindingsError) -> Self {
        match e {
            FindingsError::ScanNotFound => ReportsError::ScanNotFound,
            other => ReportsError::Findings(other.to_string()),
        }
    }
}

impl From<ReportsError> for AppError {
    fn from(e: ReportsError) -> Self {
        match e {
            ReportsError::InvalidInput(f) => AppError::InvalidInput(f.into()),
            ReportsError::ScanNotFound => AppError::ScanNotFound,
            ReportsError::Findings(m) => AppError::Internal(format!("findings:{m}")),
            ReportsError::OutputWrite => AppError::ReportOutputWrite,
            ReportsError::PdfRender(m) => AppError::ReportPdfRender(m),
            ReportsError::AutoExportCopy => AppError::ReportAutoExportCopy,
            ReportsError::Db(m) => AppError::Db(m),
            ReportsError::Io(m) => AppError::Io(m),
        }
    }
}
