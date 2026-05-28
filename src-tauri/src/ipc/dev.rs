// Local-only demo data seeder.
//
// PR #64 — local QA helper. The dev build of CloudSaw doesn't ship a
// bundled ScoutSuite binary (see CloudSaw-Local-Run.md §6), so a
// developer running `npm run tauri dev` against a fresh data root
// has no realistic way to exercise the Findings UI without setting
// up a real AWS account and dropping a verified ScoutSuite binary
// at the expected path. This command short-circuits that: it
// fabricates one synthetic finding per bundled knowledge-base
// article, cycling severities across the set so every UI tier
// (Critical / High / Medium / Low / Informational) is populated.
//
// The fabricated data lives only in the user's local SQLite — there
// is no fixture file in the repo, no network traffic, and nothing
// transmitted anywhere. The user can delete the synthetic scan via
// the existing Findings → "Delete scan" flow when they're done.
//
// Compile model:
//   - The function is compiled into BOTH dev and release builds so the
//     IPC handler list in `lib.rs::run` can reference it unconditionally.
//   - The implementation body is gated by `#[cfg(debug_assertions)]`.
//     Release builds reject the call with `AppError::Internal`. There
//     is no UI hook in production (the `__cloudsaw_dev` console hook
//     in App.tsx is gated by Vite's `import.meta.env.DEV` and stripped
//     from release bundles).

use crate::errors::AppError;
use crate::findings::ParseSummary;

#[tauri::command]
pub fn dev_seed_demo_findings(aws_account_id: String) -> Result<ParseSummary, AppError> {
    #[cfg(not(debug_assertions))]
    {
        let _ = aws_account_id;
        return Err(AppError::Internal(
            "dev_seed_demo_findings is only available in development builds".to_string(),
        ));
    }
    #[cfg(debug_assertions)]
    {
        seed(aws_account_id)
    }
}

#[cfg(debug_assertions)]
fn seed(aws_account_id: String) -> Result<ParseSummary, AppError> {
    use std::collections::BTreeSet;

    use chrono::Utc;

    use crate::findings::parser::{ParsedFinding, ParsedResource, ParsedScoutOutput};
    use crate::findings::Severity;
    use crate::knowledgebase;
    use crate::scanner::storage as scanner_storage;

    // 1. Enumerate every bundled KB article. Each `finding_id` here is
    //    really a rule_key (the KB module's historical naming); the
    //    Finding type's account-scoped `finding_id` is derived from
    //    `sha256(aws_account_id:rule_key)` downstream.
    let articles = knowledgebase::list_articles()
        .map_err(|e| AppError::Internal(format!("dev_seed: list_articles failed: {e}")))?;
    if articles.is_empty() {
        return Err(AppError::Internal(
            "dev_seed: no KB articles are bundled — nothing to seed".to_string(),
        ));
    }

    // 2. Create a synthetic scan record. The `cloudsaw-scan-<short>`
    //    role-session-name shape matches the real scanner's so any
    //    CloudTrail-style correlation logic the UI applies still sees
    //    a well-formed value.
    let scan_id = format!("demo-{}", Utc::now().timestamp_millis());
    let role_session_name = format!("cloudsaw-scan-{}", &scan_id);
    scanner_storage::insert_pending(&scan_id, &aws_account_id, &role_session_name)
        .map_err(|e| AppError::Internal(format!("dev_seed: insert_pending: {e}")))?;
    scanner_storage::record_complete(&scan_id, None, false)
        .map_err(|e| AppError::Internal(format!("dev_seed: record_complete: {e}")))?;

    // 3. Build a synthetic ParsedScoutOutput from the KB list. One
    //    finding per article, cycling severities so the Findings UI
    //    shows every tier.
    let severities = [
        Severity::Critical,
        Severity::High,
        Severity::Medium,
        Severity::Low,
        Severity::Informational,
    ];
    let mut findings: Vec<ParsedFinding> = Vec::with_capacity(articles.len());
    let mut services_scanned: BTreeSet<String> = BTreeSet::new();
    for (idx, article) in articles.iter().enumerate() {
        let rule_key = article.finding_id.clone();
        // Service is the prefix before the first `-`. Falls back to
        // "other" for the rare article whose filename doesn't follow
        // the `service-rest` convention.
        let service = rule_key
            .split_once('-')
            .map(|(svc, _)| svc.to_string())
            .unwrap_or_else(|| "other".to_string());
        services_scanned.insert(service.clone());
        let severity = severities[idx % severities.len()];
        let flagged = ((idx % 5) + 1) as i64;
        let resources: Vec<ParsedResource> = (0..flagged)
            .map(|n| ParsedResource {
                resource_path: format!("{service}.resources.id.demo-resource-{}", n + 1),
                invalid: false,
            })
            .collect();
        findings.push(ParsedFinding {
            rule_key: rule_key.clone(),
            raw_type: rule_key.clone(),
            service: service.clone(),
            severity,
            description: format!("Demo finding for {}", article.title),
            rationale: Some(
                "Synthetic demo data generated by dev_seed_demo_findings — no real AWS \
                 resources are involved. Delete via Findings → 'Delete scan' when finished."
                    .to_string(),
            ),
            dashboard_name: Some("Demo Scan".to_string()),
            resource_path_pattern: Some(format!("{service}.resources.id")),
            checked_items: flagged + 10,
            flagged_items: flagged,
            resources,
        });
    }
    let parsed = ParsedScoutOutput {
        account_id: Some(aws_account_id.clone()),
        services_scanned,
        findings,
        unknown_severity_count: 0,
        unknown_type_count: 0,
    };

    // 4. Apply through the same storage path a real scan uses.
    let scan = scanner_storage::get(&scan_id)
        .map_err(|e| AppError::Internal(format!("dev_seed: get scan: {e}")))?;
    crate::findings::storage::apply_parsed(&scan, &parsed)
        .map_err(|e| AppError::Internal(format!("dev_seed: apply_parsed: {e}")))
}
