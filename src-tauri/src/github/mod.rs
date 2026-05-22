// GitHub integration — Contract 12.
//
// One PAT, two surfaces:
//
//   * 12A. Error reporting: redacted diagnostic bundle + direct API
//     submission to the CloudSaw repo, with a browser fallback that
//     ALWAYS works (with or without a token).
//   * 12B. Findings → GitHub Issues: per-finding "Create ticket" against
//     a user-selected repo; local link stored so the UI shows "tracked
//     in #N".
//
// Shared invariants (Contract 12 §Constraints + §Security Check):
//
//   * The PAT lives ONLY in the OS keychain at `cloudsaw.github_pat`.
//     It is fetched on demand and held in memory minimally
//     (`zeroize::Zeroizing<String>`).
//   * Before any direct API submission, the IPC layer builds a full
//     `IssuePreview`; the UI shows it to the user; submission proceeds
//     only when the user confirms.
//   * The browser fallback is always available, even when a token is
//     configured.
//   * The diagnostic bundle is redacted (account IDs masked, ARNs
//     truncated, no credentials/tokens/keys, secret-keyword lines
//     dropped entirely).
//   * Every ticket creation records an event-log entry.
//   * The integration is one-way — CloudSaw creates issues; it does not
//     subscribe to webhooks or poll issue state in this version.

pub mod bundle;
pub mod client;
pub mod error;
pub mod pat;
pub mod redact;
pub mod storage;
pub mod types;

pub use error::GithubError;
pub use types::{
    BrowserSubmission, DiagnosticBundle, FindingTicket, GithubSettings, IssueCreated, IssuePreview,
    RepoSelection, TokenStatus,
};

use zeroize::Zeroizing;

use crate::eventlog::{self, EventInput, EventKind};
use crate::findings;

// The CloudSaw repository error reports land on. Hard-coded so the
// destination is never inferred from user input (CLAUDE.md §5 hard-
// DO-NOT against accepting arbitrary URLs from the UI). Exposed to
// callers via `error_report_repo()` which returns an owned
// `RepoSelection` so the IPC payload is self-contained.
const CLOUDSAW_REPO_OWNER: &str = "Camdun-Solutions";
const CLOUDSAW_REPO_NAME: &str = "CloudSaw";

/// Security contact for sensitive disclosures. Displayed by the error
/// dialog as the channel for issues that may involve sensitive AWS data
/// (Contract 12 §Constraints).
pub const SECURITY_CONTACT: &str = "security@cloud-saw.com";

/// Build the typed `RepoSelection` representing the CloudSaw repo.
/// Function rather than const so we can return the owned strings.
pub fn error_report_repo() -> RepoSelection {
    RepoSelection {
        owner: CLOUDSAW_REPO_OWNER.to_string(),
        name: CLOUDSAW_REPO_NAME.to_string(),
    }
}

// --- Settings surface ----------------------------------------------------

pub fn get_settings() -> Result<GithubSettings, GithubError> {
    Ok(GithubSettings {
        token: TokenStatus {
            configured: pat::get()?.is_some(),
        },
        findings_repo: storage::get_findings_repo()?,
        error_report_repo: error_report_repo(),
        security_contact: SECURITY_CONTACT.to_string(),
    })
}

pub fn set_token(value: String) -> Result<(), GithubError> {
    pat::set(Zeroizing::new(value))?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        "GitHub token configured.",
    ));
    Ok(())
}

pub fn clear_token() -> Result<(), GithubError> {
    pat::clear()?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        "GitHub token cleared.",
    ));
    Ok(())
}

pub fn set_findings_repo(repo: Option<RepoSelection>) -> Result<(), GithubError> {
    storage::set_findings_repo(repo.as_ref())?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        match &repo {
            Some(r) => format!("Findings-ticket repo set to {}.", r.as_path()),
            None => "Findings-ticket repo cleared.".to_string(),
        },
    ));
    Ok(())
}

/// URL of the GitHub fine-grained-token settings page. Surfaced via IPC
/// so the Settings "Generate token" button opens it in the browser.
pub fn generate_token_url() -> &'static str {
    "https://github.com/settings/personal-access-tokens/new"
}

// --- Error-reporting surface --------------------------------------------

/// Build the preview shown to the user BEFORE any direct submission.
/// Same content used by the browser fallback's prefilled URL.
pub fn prepare_error_report(
    notes: Option<String>,
    locale: &str,
) -> Result<IssuePreview, GithubError> {
    let bundle = bundle::build_capped(notes, locale)?;
    let title = error_report_title(&bundle);
    let body = bundle.to_issue_body();
    Ok(IssuePreview {
        repo: error_report_repo(),
        title,
        body,
        labels: vec!["bug".to_string(), "from-app".to_string()],
        bundle,
    })
}

/// Submit an error report via the GitHub API. Requires a configured PAT.
/// The `preview` argument is the EXACT content displayed to the user
/// — the UI passes it back unchanged so what the user reviewed is what
/// gets submitted (no last-mile rewriting).
pub fn submit_error_report(preview: &IssuePreview) -> Result<IssueCreated, GithubError> {
    let created = client::create_issue(
        &preview.repo,
        &preview.title,
        &preview.body,
        &preview.labels,
    )?;
    eventlog::record_event(
        EventInput::new(
            EventKind::GithubTicketCreated,
            format!(
                "Filed error report on {}#{}",
                preview.repo.as_path(),
                created.issue_number,
            ),
        )
        .with_path(created.issue_url.clone())
        .with_item_count(created.issue_number as i64),
    );
    Ok(created)
}

/// Browser-fallback URL for an error report. Always available — does
/// NOT require a token. Used both when no token is configured AND as a
/// per-report choice when a token IS configured.
pub fn browser_fallback_for_error_report(preview: &IssuePreview) -> BrowserSubmission {
    BrowserSubmission {
        url: client::browser_fallback_url(
            &preview.repo,
            &preview.title,
            &preview.body,
            &preview.labels,
        ),
    }
}

fn error_report_title(bundle: &DiagnosticBundle) -> String {
    let notes_summary = bundle
        .notes
        .as_deref()
        .map(|n| n.lines().next().unwrap_or("").trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > 80 {
                let truncated: String = s.chars().take(77).collect();
                format!("{truncated}…")
            } else {
                s.to_string()
            }
        });
    match notes_summary {
        Some(summary) => format!("[CloudSaw] {summary}"),
        None => format!(
            "[CloudSaw] Error report from {} {}",
            bundle.os_family, bundle.app_version
        ),
    }
}

// --- Findings → tickets surface -----------------------------------------

/// Build the issue preview for a finding ticket. The UI shows this in
/// the same submission-preview modal as the error report path.
pub fn prepare_finding_ticket(
    finding_id: &str,
    repo: &RepoSelection,
) -> Result<IssuePreview, GithubError> {
    storage::validate_repo(repo)?;
    let detail =
        findings::get_finding(finding_id).map_err(|_| GithubError::InvalidInput("finding_id"))?;
    let kb = crate::knowledgebase::get_article(finding_id).ok();

    let title = format!(
        "[CloudSaw] {severity}: {rule}",
        severity = detail.finding.severity.as_str(),
        rule = detail.finding.rule_key,
    );

    let mut body = String::new();
    body.push_str("## Finding\n\n");
    body.push_str(&format!(
        "- **Rule:** `{rule}`\n",
        rule = detail.finding.rule_key
    ));
    body.push_str(&format!(
        "- **Severity:** {sev}\n",
        sev = detail.finding.severity.as_str()
    ));
    body.push_str(&format!(
        "- **Service:** {svc}\n",
        svc = detail.finding.service
    ));
    body.push_str(&format!(
        "- **Account:** {acct}\n",
        acct = crate::accounts::mask_for_logs(&detail.finding.aws_account_id)
    ));
    body.push_str(&format!(
        "- **Checked / flagged:** {checked} / {flagged}\n",
        checked = detail.finding.checked_items,
        flagged = detail.finding.flagged_items
    ));
    body.push_str(&format!(
        "- **First seen:** {first}\n",
        first = detail.finding.first_seen_at.to_rfc3339()
    ));
    body.push_str(&format!(
        "- **Last seen:** {last}\n",
        last = detail.finding.last_seen_at.to_rfc3339()
    ));
    body.push_str("\n## Description\n\n");
    body.push_str(&redact::redact_block(&detail.finding.description));
    body.push_str("\n\n");
    if let Some(rationale) = detail.finding.rationale.as_deref() {
        body.push_str("## Rationale\n\n");
        body.push_str(&redact::redact_block(rationale));
        body.push_str("\n\n");
    }
    if let Some(article) = kb {
        if article.matched {
            body.push_str("## Remediation\n\n");
            body.push_str(&redact::redact_block(&article.remediation));
            body.push_str("\n\n");
            if !article.terraform_fix.trim().is_empty() {
                body.push_str("## Terraform fix\n\n");
                body.push_str(&redact::redact_block(&article.terraform_fix));
                body.push_str("\n\n");
            }
            if !article.aws_cli_fix.trim().is_empty() {
                body.push_str("## AWS CLI fix\n\n");
                body.push_str(&redact::redact_block(&article.aws_cli_fix));
                body.push_str("\n\n");
            }
        }
    }
    if !detail.resources.is_empty() {
        body.push_str(&format!(
            "## Affected resources ({n})\n\n",
            n = detail.resources.len()
        ));
        for r in detail.resources.iter().take(50) {
            let masked = redact::redact_line(&r.resource_path);
            body.push_str(&format!("- `{masked}`\n"));
        }
        body.push_str("\n");
    }
    body.push_str("---\n\nFiled from CloudSaw. Account IDs are masked; ARNs are truncated.\n");

    let labels = vec![
        "cloudsaw-finding".to_string(),
        format!("severity:{}", detail.finding.severity.as_str()),
    ];

    let bundle = DiagnosticBundle {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os_family: std::env::consts::FAMILY.to_string(),
        os_release: std::env::consts::OS.to_string(),
        locale: "en".to_string(),
        generated_at: chrono::Utc::now(),
        redacted_log_lines: Vec::new(),
        notes: None,
    };

    Ok(IssuePreview {
        repo: repo.clone(),
        title,
        body,
        labels,
        bundle,
    })
}

/// Submit a finding ticket. Refuses to file if the finding already has
/// a linked ticket (Contract 12 §Edge Cases). On success persists the
/// link and emits a `GithubTicketCreated` event.
pub fn submit_finding_ticket(
    finding_id: &str,
    preview: &IssuePreview,
) -> Result<FindingTicket, GithubError> {
    if storage::get_finding_ticket(finding_id)?.is_some() {
        return Err(GithubError::DuplicateTicket);
    }
    let detail =
        findings::get_finding(finding_id).map_err(|_| GithubError::InvalidInput("finding_id"))?;
    let created = client::create_issue(
        &preview.repo,
        &preview.title,
        &preview.body,
        &preview.labels,
    )?;
    let ticket = storage::upsert_finding_ticket(
        finding_id,
        &detail.finding.aws_account_id,
        &created.repo,
        created.issue_number,
        &created.issue_url,
    )?;
    eventlog::record_event(
        EventInput::new(
            EventKind::GithubTicketCreated,
            format!(
                "Filed finding ticket on {}#{}",
                created.repo.as_path(),
                created.issue_number,
            ),
        )
        .with_account(detail.finding.aws_account_id.clone())
        .with_path(created.issue_url.clone())
        .with_item_count(created.issue_number as i64),
    );
    Ok(ticket)
}

pub fn browser_fallback_for_finding_ticket(preview: &IssuePreview) -> BrowserSubmission {
    BrowserSubmission {
        url: client::browser_fallback_url(
            &preview.repo,
            &preview.title,
            &preview.body,
            &preview.labels,
        ),
    }
}

pub fn get_finding_ticket(finding_id: &str) -> Result<Option<FindingTicket>, GithubError> {
    storage::get_finding_ticket(finding_id)
}

pub fn list_finding_tickets(aws_account_id: &str) -> Result<Vec<FindingTicket>, GithubError> {
    if !is_valid_account_id(aws_account_id) {
        return Err(GithubError::InvalidInput("aws_account_id"));
    }
    storage::list_tickets_for_account(aws_account_id, 200)
}

fn is_valid_account_id(id: &str) -> bool {
    id.len() == 12 && id.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_report_repo_returns_canonical_cloudsaw_coordinates() {
        let r = error_report_repo();
        assert_eq!(r.owner, "Camdun-Solutions");
        assert_eq!(r.name, "CloudSaw");
    }

    #[test]
    fn security_contact_is_the_documented_address() {
        // QA item: "The error dialog displays security@cloud-saw.com
        // for sensitive reports." This constant is what the IPC layer
        // surfaces.
        assert_eq!(SECURITY_CONTACT, "security@cloud-saw.com");
    }

    #[test]
    fn error_report_title_uses_notes_when_present() {
        let bundle = DiagnosticBundle {
            app_version: "x".into(),
            os_family: "x".into(),
            os_release: "x".into(),
            locale: "x".into(),
            generated_at: chrono::Utc::now(),
            redacted_log_lines: vec![],
            notes: Some("clicked Scan, app froze".into()),
        };
        let t = error_report_title(&bundle);
        assert!(t.contains("clicked Scan"));
    }

    #[test]
    fn error_report_title_falls_back_to_app_and_os_when_no_notes() {
        let bundle = DiagnosticBundle {
            app_version: "2026.5.0".into(),
            os_family: "windows".into(),
            os_release: "windows".into(),
            locale: "en".into(),
            generated_at: chrono::Utc::now(),
            redacted_log_lines: vec![],
            notes: None,
        };
        let t = error_report_title(&bundle);
        assert!(t.contains("2026.5.0"));
        assert!(t.contains("windows"));
    }
}
