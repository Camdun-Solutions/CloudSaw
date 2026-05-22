// Typed error enum for the GitHub integration. CLAUDE.md §4.2: no raw
// reqwest/SDK text crosses IPC. Each variant maps to a stable code so
// the frontend can localize the message.

use thiserror::Error;

use crate::errors::AppError;

#[derive(Debug, Error)]
pub enum GithubError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// No PAT has been configured. The UI uses this to surface the
    /// "Configure GitHub token" action.
    #[error("no token")]
    NoToken,

    /// The configured PAT was rejected by GitHub (revoked, expired,
    /// insufficient scope). Distinct from `Network` so the UI can route
    /// the user to Settings.
    #[error("token invalid or expired")]
    TokenInvalid,

    /// GitHub responded with a rate-limit signal. The UI offers a retry
    /// or the browser fallback.
    #[error("rate limited")]
    RateLimited,

    /// Generic transport failure (connect refused, TLS handshake, …).
    /// We deliberately collapse the dozens of reqwest sub-kinds into one
    /// surface — the user-visible advice is the same.
    #[error("network unreachable")]
    Network,

    /// GitHub accepted the request but returned an unexpected status or
    /// response body. Carries the numeric status so the UI can display
    /// "GitHub returned 422" without inventing one.
    #[error("github error status {0}")]
    Server(u16),

    /// User has not selected a findings-ticket destination repo yet.
    #[error("no findings repo configured")]
    NoFindingsRepo,

    /// A finding already has a linked ticket — the caller (the
    /// "Create ticket" action) should show the existing link rather
    /// than file a duplicate.
    #[error("ticket already exists")]
    DuplicateTicket,

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl GithubError {
    pub fn code(&self) -> &'static str {
        match self {
            GithubError::InvalidInput(_) => "invalid_input",
            GithubError::NoToken => "github_no_token",
            GithubError::TokenInvalid => "github_token_invalid",
            GithubError::RateLimited => "github_rate_limited",
            GithubError::Network => "github_network",
            GithubError::Server(_) => "github_server_error",
            GithubError::NoFindingsRepo => "github_no_findings_repo",
            GithubError::DuplicateTicket => "github_duplicate_ticket",
            GithubError::Db(_) => "db_error",
            GithubError::Io(_) => "io_error",
        }
    }
}

impl From<rusqlite::Error> for GithubError {
    fn from(e: rusqlite::Error) -> Self {
        GithubError::Db(e.to_string())
    }
}

impl From<GithubError> for AppError {
    fn from(e: GithubError) -> Self {
        match e {
            GithubError::InvalidInput(f) => AppError::InvalidInput(f.into()),
            GithubError::NoToken => AppError::GithubNoToken,
            GithubError::TokenInvalid => AppError::GithubTokenInvalid,
            GithubError::RateLimited => AppError::GithubRateLimited,
            GithubError::Network => AppError::GithubNetwork,
            GithubError::Server(status) => AppError::GithubServerError(status),
            GithubError::NoFindingsRepo => AppError::GithubNoFindingsRepo,
            GithubError::DuplicateTicket => AppError::GithubDuplicateTicket,
            GithubError::Db(m) => AppError::Db(m),
            GithubError::Io(m) => AppError::Io(m),
        }
    }
}
