// Public types for the GitHub integration. None of these carry secret
// material — the PAT lives only in the OS keychain (see `pat` module).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A repo identifier as the user supplies it in Settings: `owner/name`
/// (e.g. `acme/cloud-infra`). Validated inside the module before any
/// API call or DB write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSelection {
    pub owner: String,
    pub name: String,
}

impl RepoSelection {
    pub fn as_path(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Status of the configured PAT, used by the Settings UI to decide which
/// affordances to render. The token VALUE never crosses IPC — only whether
/// one exists and what scope-level check we last performed.
#[derive(Debug, Clone, Serialize)]
pub struct TokenStatus {
    pub configured: bool,
}

/// Status of the entire GitHub integration: whether a token is set and
/// the configured findings-ticket destination (if any).
#[derive(Debug, Clone, Serialize)]
pub struct GithubSettings {
    pub token: TokenStatus,
    pub findings_repo: Option<RepoSelection>,
    /// The hard-coded CloudSaw repo error reports land on. Exposed via IPC
    /// so the UI can display it in the error dialog.
    pub error_report_repo: RepoSelection,
    /// The security contact for sensitive reports. Static; exposed via
    /// IPC so the error dialog renders the canonical value.
    pub security_contact: String,
}

/// The "what will be submitted" preview shown to the user before any
/// direct API call. Contract 12 §Constraints + §Acceptance Criteria
/// require that this exact content (bundle, title, body) be displayed
/// before submission and that submission proceed only on explicit
/// action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuePreview {
    pub repo: RepoSelection,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// Surfaced separately so the UI can render it with its own affordance
    /// ("Copy bundle") and so a future export-to-file action doesn't have
    /// to re-derive it.
    pub bundle: DiagnosticBundle,
}

/// The diagnostic bundle attached to an error report. CLAUDE.md §4.4 +
/// Contract 12 §Constraints: every account ID is masked, every ARN
/// truncated, every credential/token/key removed. The bundle is shown
/// to the user before any submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticBundle {
    pub app_version: String,
    pub os_family: String,
    pub os_release: String,
    pub locale: String,
    pub generated_at: DateTime<Utc>,
    pub redacted_log_lines: Vec<String>,
    /// Free-form caller-supplied notes (the "what were you doing?" field
    /// in the error dialog). Redacted by the same rules as logs before
    /// it lands here — but the redaction surface is bounded: secrets
    /// the user pastes themselves get the same masking as anything else.
    pub notes: Option<String>,
}

impl DiagnosticBundle {
    /// Render the bundle as the body of a GitHub issue (markdown). Pure;
    /// no IO. The frontend renders the same text in the preview modal,
    /// so what the user sees IS what's submitted.
    pub fn to_issue_body(&self) -> String {
        let mut s = String::new();
        s.push_str("## CloudSaw diagnostic bundle\n\n");
        s.push_str(&format!("- **App version:** {}\n", self.app_version));
        s.push_str(&format!(
            "- **OS:** {} ({})\n",
            self.os_family, self.os_release
        ));
        s.push_str(&format!("- **Locale:** {}\n", self.locale));
        s.push_str(&format!("- **Generated at:** {}\n", self.generated_at.to_rfc3339()));
        s.push_str("\n");
        if let Some(notes) = &self.notes {
            if !notes.trim().is_empty() {
                s.push_str("## What you were doing\n\n");
                s.push_str(notes);
                s.push_str("\n\n");
            }
        }
        s.push_str("## Redacted log excerpt\n\n");
        s.push_str("Account IDs are masked to the last 4 digits; ARNs are truncated; no credentials, tokens, or API keys are included.\n\n");
        s.push_str("```\n");
        for line in &self.redacted_log_lines {
            s.push_str(line);
            s.push('\n');
        }
        s.push_str("```\n");
        s
    }
}

/// The locally-stored link between a finding and the GitHub issue the
/// user filed for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingTicket {
    pub finding_id: String,
    pub aws_account_id_masked: String,
    pub repo: RepoSelection,
    pub issue_number: u32,
    pub issue_url: String,
    pub created_at: DateTime<Utc>,
}

/// What a successful direct submission returns to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct IssueCreated {
    pub repo: RepoSelection,
    pub issue_number: u32,
    pub issue_url: String,
}

/// Discriminated result for the "what URL should we open?" browser
/// fallback. The UI opens this URL in the system browser; GitHub
/// pre-fills the new-issue form from the query string.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserSubmission {
    pub url: String,
}
