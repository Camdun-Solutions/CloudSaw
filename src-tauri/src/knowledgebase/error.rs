// KnowledgebaseError — typed enum returned by every public
// `knowledgebase::*` function.
//
// Each variant maps to a stable IPC error code via `code()` and folds into
// `AppError` for serialization. CLAUDE.md §4.2: no raw network text, no
// credential material — these errors only carry stable tags or local
// filenames.

use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgebaseError {
    /// Caller-side validation failure (empty finding_id, invalid framework
    /// slug, etc.). The inner string is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Two bundled or two remote articles claim the same `finding_id`.
    /// Contract 08 §Constraints: ambiguity must surface, not be papered
    /// over. The inner string is the duplicated id.
    #[error("duplicate article id: {0}")]
    DuplicateArticleId(String),

    /// The bundled or remote mappings document failed JSON parsing.
    #[error("malformed mappings document: {0}")]
    MalformedMappings(String),

    /// A remote refresh was attempted while the feature is disabled.
    /// `check_for_kb_update` and `apply_kb_update` both refuse rather than
    /// silently flipping the toggle.
    #[error("remote refresh disabled")]
    RefreshDisabled,

    /// A remote refresh request failed to reach the upstream repo.
    #[error("remote refresh unreachable")]
    RefreshUnreachable,

    /// The remote refresh fetched content that failed validation (wrong
    /// shape, missing required fields, duplicate article IDs, etc.). The
    /// bundled baseline is left intact.
    #[error("remote refresh content invalid")]
    RefreshInvalidContent,

    /// The remote refresh fetched content whose declared version matched
    /// the one already cached. `check_for_kb_update` returns this as a
    /// non-error informational state; `apply_kb_update` may also surface
    /// it on a redundant re-apply.
    #[error("remote refresh already up to date")]
    RefreshUpToDate,

    /// Filesystem failure while reading the bundled cache or writing the
    /// remote cache.
    #[error("io: {0}")]
    Io(String),

    /// SQLite operation failed (settings read/write).
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated. Stable source-code tag, never raw text
    /// from a third party.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl KnowledgebaseError {
    pub fn code(&self) -> &'static str {
        match self {
            KnowledgebaseError::InvalidInput(_) => "invalid_input",
            KnowledgebaseError::DuplicateArticleId(_) => "kb_duplicate_article_id",
            KnowledgebaseError::MalformedMappings(_) => "kb_malformed_mappings",
            KnowledgebaseError::RefreshDisabled => "kb_refresh_disabled",
            KnowledgebaseError::RefreshUnreachable => "kb_refresh_unreachable",
            KnowledgebaseError::RefreshInvalidContent => "kb_refresh_invalid_content",
            KnowledgebaseError::RefreshUpToDate => "kb_refresh_up_to_date",
            KnowledgebaseError::Io(_) => "io_error",
            KnowledgebaseError::Db(_) => "db_error",
            KnowledgebaseError::Internal(_) => "internal_error",
        }
    }
}

impl From<std::io::Error> for KnowledgebaseError {
    fn from(e: std::io::Error) -> Self {
        KnowledgebaseError::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for KnowledgebaseError {
    fn from(e: rusqlite::Error) -> Self {
        KnowledgebaseError::Db(e.to_string())
    }
}

impl From<KnowledgebaseError> for AppError {
    fn from(err: KnowledgebaseError) -> Self {
        match err {
            KnowledgebaseError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            KnowledgebaseError::DuplicateArticleId(id) => {
                AppError::Config(format!("knowledgebase: duplicate article id '{id}'"))
            }
            KnowledgebaseError::MalformedMappings(msg) => {
                AppError::Config(format!("knowledgebase: {msg}"))
            }
            KnowledgebaseError::RefreshDisabled => AppError::KbRefreshDisabled,
            KnowledgebaseError::RefreshUnreachable => AppError::KbRefreshUnreachable,
            KnowledgebaseError::RefreshInvalidContent => AppError::KbRefreshInvalidContent,
            KnowledgebaseError::RefreshUpToDate => AppError::KbRefreshUpToDate,
            KnowledgebaseError::Io(s) => AppError::Io(s),
            KnowledgebaseError::Db(s) => AppError::Db(s),
            KnowledgebaseError::Internal(tag) => AppError::Internal(format!("kb:{tag}")),
        }
    }
}
