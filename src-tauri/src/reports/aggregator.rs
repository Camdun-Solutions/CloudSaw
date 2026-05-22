// Aggregator — reads findings, scans, KB articles, mappings, and
// events into a single `ReportContent` value. Pure-ish: every input
// comes from a public module API; the only side effect is the SQLite
// reads those APIs perform.
//
// Disclosure: the caller passes `AccountIdDisclosure` so the
// renderer never has to know about masking. Every account-shaped
// string in `ReportContent` is pre-rendered to the chosen disclosure
// mode; the renderer writes verbatim. Contract 15 §Constraints +
// §Acceptance Criteria require that full IDs appear only on
// explicit user opt-in.

use chrono::{DateTime, Duration, Utc};

use super::error::ReportsError;
use super::model::{
    AccountIdDisclosure, AccountRef, EventRow, FindingRow, PerServiceTotals, ReportContent,
    ReportHeader, ReportKind, ScanSummary, SeverityCounts,
};
use crate::accounts::{self, mask_for_logs};
use crate::eventlog::{self, EventKind, EventLogFilter};
use crate::findings::{self, FindingsFilter, Severity};
use crate::knowledgebase;

const REVIEW_BANNER: &str =
    "Review this report for sensitive data before sharing. Account IDs may be \
     masked or shown in full depending on the export choice; resource paths and \
     timestamps reflect the underlying scan and are not redacted further.";

const REPORT_LOCALE_DEFAULT: &str = "en";

/// Cap on the number of resource paths embedded per finding. Larger
/// findings show a "+N more" suffix in `truncated_extra` so report
/// size stays bounded.
const RESOURCE_CAP_PER_FINDING: usize = 50;

/// Cap on the number of findings emitted by a custom report so a
/// runaway date range can't blow out memory. The HTML renderer
/// stays linear; the cap is a defense-in-depth bound.
const CUSTOM_FINDING_CAP: usize = 5_000;

/// Build the per-scan report (Contract 15A).
pub fn build_per_scan(
    scan_id: &str,
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, ReportsError> {
    let scan = findings::get_scan(scan_id)?;
    let account = lookup_account_ref(&scan.aws_account_id, disclosure);

    let raw_findings = findings::list_findings(
        scan_id,
        FindingsFilter {
            limit: Some((CUSTOM_FINDING_CAP as i64).min(5_000)),
            ..Default::default()
        },
    )?;

    let mut counts = SeverityCounts::empty();
    let mut findings_out: Vec<FindingRow> = Vec::with_capacity(raw_findings.len());
    let mut service_index: std::collections::BTreeMap<String, (usize, SeverityCounts)> =
        std::collections::BTreeMap::new();

    for f in raw_findings.iter() {
        counts.bump(f.severity);
        let row = build_finding_row(f, disclosure)?;
        let entry = service_index
            .entry(f.service.clone())
            .or_insert((0, SeverityCounts::empty()));
        entry.0 += 1;
        entry.1.bump(f.severity);
        findings_out.push(row);
    }

    findings_out.sort_by(|a, b| {
        severity_rank(a.severity)
            .cmp(&severity_rank(b.severity))
            .then(b.last_seen_at.cmp(&a.last_seen_at))
    });

    let per_service: Vec<PerServiceTotals> = service_index
        .into_iter()
        .map(|(service, (n, sc))| PerServiceTotals {
            service,
            findings: n,
            severity_counts: sc,
        })
        .collect();

    let summary = ScanSummary {
        scan_id: scan.scan_id.clone(),
        account: account.clone(),
        started_at: scan.started_at,
        finished_at: scan.finished_at,
        status: scan_status_label(&scan.status),
        severity_counts: counts,
    };

    let empty_state = if findings_out.is_empty() {
        Some(
            "This scan reported zero findings. Either the scanner observed no \
             misconfigurations, or the scanner role lacked the permissions needed \
             to see them. A clean scan and a permission-limited scan can look \
             identical — review the scanner role's policy if you expected findings."
                .to_string(),
        )
    } else {
        None
    };

    Ok(ReportContent {
        header: ReportHeader {
            kind: ReportKind::PerScan,
            title: format!("Scan report — {}", scan.scan_id),
            subtitle: Some(format!(
                "{} (started {})",
                account.label,
                scan.started_at.to_rfc3339()
            )),
            generated_at: Utc::now(),
            cloudsaw_version: env!("CARGO_PKG_VERSION").to_string(),
            review_banner: REVIEW_BANNER.to_string(),
            disclosure,
            locale: REPORT_LOCALE_DEFAULT.to_string(),
        },
        scans: vec![summary],
        findings: findings_out,
        per_service,
        events: vec![],
        empty_state_note: empty_state,
    })
}

/// Build a custom-range report (Contract 15B). Scopes to the supplied
/// list of accounts so the report never ambiguously mixes accounts
/// the user didn't ask for.
pub fn build_custom(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    account_scope: &[String],
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, ReportsError> {
    if end < start {
        return Err(ReportsError::InvalidInput("date_range"));
    }
    if end - start > Duration::days(366 * 5) {
        // Five-year cap: any longer range almost certainly indicates
        // a UI bug, and exceeds what the in-memory cap can serve.
        return Err(ReportsError::InvalidInput("date_range"));
    }

    let accounts_in_scope: Vec<String> = if account_scope.is_empty() {
        // Empty == "all accounts known locally."
        accounts::list_accounts()
            .map_err(|e| ReportsError::Db(e.to_string()))?
            .into_iter()
            .map(|a| a.aws_account_id)
            .collect()
    } else {
        for id in account_scope {
            if id.len() != 12 || !id.chars().all(|c| c.is_ascii_digit()) {
                return Err(ReportsError::InvalidInput("account_id"));
            }
        }
        account_scope.to_vec()
    };

    let mut scan_summaries: Vec<ScanSummary> = Vec::new();
    let mut findings_out: Vec<FindingRow> = Vec::new();
    let mut counts = SeverityCounts::empty();
    let mut service_index: std::collections::BTreeMap<String, (usize, SeverityCounts)> =
        std::collections::BTreeMap::new();
    let mut seen_finding_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for account_id in &accounts_in_scope {
        let account_ref = lookup_account_ref(account_id, disclosure);
        let scans = findings::list_scans(account_id).map_err(ReportsError::from)?;
        for scan in scans {
            if scan.started_at < start || scan.started_at > end {
                continue;
            }
            let mut scan_counts = SeverityCounts::empty();
            let scan_findings = findings::list_findings(
                &scan.scan_id,
                FindingsFilter {
                    limit: Some(CUSTOM_FINDING_CAP as i64),
                    ..Default::default()
                },
            )?;
            for f in scan_findings.iter() {
                scan_counts.bump(f.severity);
                counts.bump(f.severity);
                // De-duplicate findings across scans — the report
                // counts the unique finding once, with its latest-seen
                // detail.
                if seen_finding_ids.insert(f.finding_id.clone()) {
                    let row = build_finding_row(f, disclosure)?;
                    let entry = service_index
                        .entry(f.service.clone())
                        .or_insert((0, SeverityCounts::empty()));
                    entry.0 += 1;
                    entry.1.bump(f.severity);
                    findings_out.push(row);
                    if findings_out.len() >= CUSTOM_FINDING_CAP {
                        break;
                    }
                }
            }
            scan_summaries.push(ScanSummary {
                scan_id: scan.scan_id.clone(),
                account: account_ref.clone(),
                started_at: scan.started_at,
                finished_at: scan.finished_at,
                status: scan_status_label(&scan.status),
                severity_counts: scan_counts,
            });
            if findings_out.len() >= CUSTOM_FINDING_CAP {
                break;
            }
        }
        if findings_out.len() >= CUSTOM_FINDING_CAP {
            break;
        }
    }

    findings_out.sort_by(|a, b| {
        severity_rank(a.severity)
            .cmp(&severity_rank(b.severity))
            .then(b.last_seen_at.cmp(&a.last_seen_at))
    });
    scan_summaries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    let per_service: Vec<PerServiceTotals> = service_index
        .into_iter()
        .map(|(service, (n, sc))| PerServiceTotals {
            service,
            findings: n,
            severity_counts: sc,
        })
        .collect();

    // Event log — restricted to the date range AND only events with
    // an account in scope (or no account, which covers cross-app events
    // like AppStarted / RetentionPurged).
    let event_filter = EventLogFilter {
        since: Some(start),
        until: Some(end),
        limit: Some(500),
        include_cleared: true,
        ..Default::default()
    };
    let events_raw = eventlog::list_events(event_filter)
        .map_err(|e| ReportsError::Db(format!("eventlog: {e}")))?;
    let accounts_mask_lookup: std::collections::HashSet<String> = accounts_in_scope
        .iter()
        .map(|id| mask_for_logs(id))
        .collect();
    let events: Vec<EventRow> = events_raw
        .into_iter()
        .filter(|e| {
            // Always keep events with no account attribution.
            match e.aws_account_id_masked.as_deref() {
                None => true,
                Some(m) => accounts_mask_lookup.contains(m),
            }
        })
        .map(|e| EventRow {
            occurred_at: e.occurred_at,
            kind: kind_label(e.kind),
            summary: e.summary,
            account_display: e.aws_account_id_masked,
            scan_id: e.scan_id,
        })
        .collect();

    let empty_state = if findings_out.is_empty() && scan_summaries.is_empty() {
        Some(
            "The selected date range contains no scans and no findings for the \
             chosen account(s)."
                .to_string(),
        )
    } else if findings_out.is_empty() {
        Some(
            "The selected date range contains scans but no findings were observed."
                .to_string(),
        )
    } else {
        None
    };

    let account_label = match accounts_in_scope.as_slice() {
        [single] => lookup_account_ref(single, disclosure).label,
        _ => format!("{} accounts", accounts_in_scope.len()),
    };

    Ok(ReportContent {
        header: ReportHeader {
            kind: ReportKind::Custom,
            title: format!(
                "Custom report — {} to {}",
                start.format("%Y-%m-%d"),
                end.format("%Y-%m-%d")
            ),
            subtitle: Some(format!("Scope: {account_label}")),
            generated_at: Utc::now(),
            cloudsaw_version: env!("CARGO_PKG_VERSION").to_string(),
            review_banner: REVIEW_BANNER.to_string(),
            disclosure,
            locale: REPORT_LOCALE_DEFAULT.to_string(),
        },
        scans: scan_summaries,
        findings: findings_out,
        per_service,
        events,
        empty_state_note: empty_state,
    })
}

// --- helpers ------------------------------------------------------------

fn build_finding_row(
    f: &findings::Finding,
    disclosure: AccountIdDisclosure,
) -> Result<FindingRow, ReportsError> {
    let detail = findings::get_finding(&f.finding_id).map_err(ReportsError::from)?;

    let article = knowledgebase::get_article(&f.rule_key).ok();
    let mapping = knowledgebase::get_control_mappings(&f.rule_key).ok();

    // Redact the account ID inside any free-form description /
    // rationale / remediation when the disclosure mode is Masked. The
    // scanner sometimes echoes account IDs verbatim in those fields;
    // the report should follow the disclosure rule end-to-end.
    let mask_text = |s: &str| match disclosure {
        AccountIdDisclosure::Full => s.to_string(),
        AccountIdDisclosure::Masked => {
            s.replace(&f.aws_account_id, &crate::accounts::mask_for_logs(&f.aws_account_id))
        }
    };

    let remediation = article
        .as_ref()
        .filter(|a| a.matched)
        .map(|a| mask_text(&a.remediation))
        .unwrap_or_default();

    let compliance_lines: Vec<String> = match mapping {
        Some(m) => m
            .frameworks
            .into_iter()
            .map(|(fid, controls)| {
                let labels: Vec<String> =
                    controls.into_iter().map(|c| c.control_id).collect();
                format!("{fid}: {}", labels.join(", "))
            })
            .collect(),
        None => Vec::new(),
    };

    let resources_iter = detail.resources.iter();
    let total = detail.resources.len();
    let resources: Vec<String> = resources_iter
        .take(RESOURCE_CAP_PER_FINDING)
        .map(|r| render_resource(&r.resource_path, &f.aws_account_id, disclosure))
        .collect();
    let truncated_extra = total.saturating_sub(resources.len());

    Ok(FindingRow {
        finding_id: f.finding_id.clone(),
        rule_key: f.rule_key.clone(),
        service: f.service.clone(),
        severity: f.severity,
        status: f.status,
        description: mask_text(&f.description),
        rationale: f.rationale.as_deref().map(mask_text),
        checked_items: f.checked_items,
        flagged_items: f.flagged_items,
        first_seen_at: f.first_seen_at,
        last_seen_at: f.last_seen_at,
        remediation,
        compliance_lines,
        resources,
        truncated_extra,
    })
}

fn lookup_account_ref(aws_account_id: &str, disclosure: AccountIdDisclosure) -> AccountRef {
    let label = accounts::get_account(aws_account_id)
        .map(|a| a.label)
        .unwrap_or_else(|_| "(unknown account)".to_string());
    AccountRef {
        display: render_account_id(aws_account_id, disclosure),
        label,
    }
}

pub fn render_account_id(id: &str, disclosure: AccountIdDisclosure) -> String {
    match disclosure {
        AccountIdDisclosure::Full => id.to_string(),
        AccountIdDisclosure::Masked => mask_for_logs(id),
    }
}

fn render_resource(
    resource_path: &str,
    aws_account_id: &str,
    disclosure: AccountIdDisclosure,
) -> String {
    match disclosure {
        AccountIdDisclosure::Full => resource_path.to_string(),
        AccountIdDisclosure::Masked => {
            // Replace any occurrence of the 12-digit account ID with
            // the masked form. We only touch the account-ID needle —
            // the rest of the ARN / resource path is left intact so
            // the report still identifies the underlying resource.
            resource_path.replace(aws_account_id, &mask_for_logs(aws_account_id))
        }
    }
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Critical => 0,
        Severity::High => 1,
        Severity::Medium => 2,
        Severity::Low => 3,
        Severity::Informational => 4,
    }
}

fn scan_status_label(status: &crate::scanner::ScanStatus) -> String {
    status.as_str().to_string()
}

fn kind_label(k: EventKind) -> String {
    k.as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_account_id_masks_by_default() {
        let s = render_account_id("111122223333", AccountIdDisclosure::Masked);
        assert_eq!(s, "****3333");
    }

    #[test]
    fn render_account_id_full_returns_verbatim() {
        let s = render_account_id("111122223333", AccountIdDisclosure::Full);
        assert_eq!(s, "111122223333");
    }

    #[test]
    fn render_resource_masks_account_id_substring() {
        let s = render_resource(
            "arn:aws:iam::111122223333:role/CloudSawScanner",
            "111122223333",
            AccountIdDisclosure::Masked,
        );
        assert_eq!(s, "arn:aws:iam::****3333:role/CloudSawScanner");
    }
}
