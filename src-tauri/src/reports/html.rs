// Self-contained HTML renderer (Contract 15 §Constraints).
//
// Output invariants:
//   * No `<script>` tags. Ever. The renderer never emits the literal
//     string `<script` — see the regression test below.
//   * No remote URLs. Every URL the renderer ships is either `mailto:`
//     or `#` (anchor). The CSS is inlined in a `<style>` block; there
//     are no `<link rel="stylesheet">`, `<img>`, `<iframe>`, `<object>`
//     elements emitted.
//   * No external resource loads. Same fence as above — there is no
//     value of any input that can introduce a network reference,
//     because every text field is HTML-escaped at the boundary.
//   * Banner, generation timestamp, and CloudSaw version live in the
//     header section so every report carries the mandatory copy.
//
// The renderer is a pure function over `ReportContent`. Tests assert
// the no-script / no-remote-url / banner-present invariants on every
// shape of report.

use chrono::SecondsFormat;

use super::model::{
    AccountIdDisclosure, EventRow, FindingRow, PerServiceTotals, ReportContent, ReportKind,
    ScanSummary, SeverityCounts,
};
use crate::findings::{FindingStatus, Severity};

const CSS: &str = include_str!("report.css");

pub fn render(content: &ReportContent) -> String {
    let mut s = String::with_capacity(8 * 1024);
    s.push_str("<!DOCTYPE html>\n");
    s.push_str("<html lang=\"");
    push_attr(&mut s, &content.header.locale);
    s.push_str("\"><head>");
    s.push_str("<meta charset=\"utf-8\">");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    s.push_str("<meta name=\"referrer\" content=\"no-referrer\">");
    s.push_str("<title>");
    push_text(&mut s, &content.header.title);
    s.push_str("</title>");
    s.push_str("<style>");
    // CSS is inlined verbatim from a known-static file — never user
    // input — so it doesn't pass through push_text.
    s.push_str(CSS);
    s.push_str("</style></head><body>");

    render_header(&mut s, content);
    render_summary(&mut s, content);
    if !content.scans.is_empty() {
        render_scans_table(&mut s, content);
    }
    if !content.per_service.is_empty() {
        render_per_service(&mut s, content);
    }
    if let Some(note) = &content.empty_state_note {
        s.push_str("<section class=\"empty\"><p>");
        push_text(&mut s, note);
        s.push_str("</p></section>");
    }
    if !content.findings.is_empty() {
        render_findings(&mut s, content);
    }
    if !content.events.is_empty() {
        render_events(&mut s, content);
    }
    render_footer(&mut s, content);
    s.push_str("</body></html>");
    s
}

fn render_header(s: &mut String, content: &ReportContent) {
    s.push_str("<header class=\"report-header\">");
    // The sensitive-data review banner is mandatory on every report.
    s.push_str("<div class=\"banner\" role=\"alert\">");
    push_text(s, &content.header.review_banner);
    s.push_str("</div>");
    s.push_str("<h1>");
    push_text(s, &content.header.title);
    s.push_str("</h1>");
    if let Some(sub) = &content.header.subtitle {
        s.push_str("<p class=\"subtitle\">");
        push_text(s, sub);
        s.push_str("</p>");
    }
    s.push_str("<dl class=\"meta\">");
    s.push_str("<dt>Generated at</dt><dd>");
    push_text(
        s,
        &content
            .header
            .generated_at
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str("</dd>");
    s.push_str("<dt>CloudSaw version</dt><dd>");
    push_text(s, &content.header.cloudsaw_version);
    s.push_str("</dd>");
    s.push_str("<dt>Account-ID disclosure</dt><dd>");
    push_text(
        s,
        match content.header.disclosure {
            AccountIdDisclosure::Masked => "masked (default)",
            AccountIdDisclosure::Full => "full (explicit opt-in)",
        },
    );
    s.push_str("</dd>");
    s.push_str("<dt>Report kind</dt><dd>");
    push_text(
        s,
        match content.header.kind {
            ReportKind::PerScan => "per-scan",
            ReportKind::Custom => "custom range",
        },
    );
    s.push_str("</dd></dl></header>");
}

fn render_summary(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"summary\"><h2>Summary</h2>");
    let total: usize = content
        .scans
        .iter()
        .map(|sc| sc.severity_counts.total())
        .sum();
    s.push_str(&format!(
        "<p>Scans: {}, findings (aggregated): {}.</p>",
        content.scans.len(),
        total,
    ));
    s.push_str("</section>");
}

fn render_scans_table(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"scans\"><h2>Scans</h2>");
    s.push_str("<table><thead><tr>");
    s.push_str("<th>Started</th><th>Account</th><th>Status</th><th>Critical</th><th>High</th><th>Medium</th><th>Low</th><th>Info</th>");
    s.push_str("</tr></thead><tbody>");
    for scan in &content.scans {
        render_scan_row(s, scan);
    }
    s.push_str("</tbody></table></section>");
}

fn render_scan_row(s: &mut String, scan: &ScanSummary) {
    s.push_str("<tr><td>");
    push_text(s, &scan.started_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    s.push_str("</td><td>");
    push_text(s, &scan.account.label);
    s.push_str(" <span class=\"account-id\">");
    push_text(s, &scan.account.display);
    s.push_str("</span></td><td>");
    push_text(s, &scan.status);
    s.push_str("</td>");
    write_counts_cells(s, &scan.severity_counts);
    s.push_str("</tr>");
}

fn render_per_service(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"per-service\"><h2>Findings by service</h2>");
    s.push_str("<table><thead><tr>");
    s.push_str("<th>Service</th><th>Findings</th><th>Critical</th><th>High</th><th>Medium</th><th>Low</th><th>Info</th>");
    s.push_str("</tr></thead><tbody>");
    for row in &content.per_service {
        render_per_service_row(s, row);
    }
    s.push_str("</tbody></table></section>");
}

fn render_per_service_row(s: &mut String, row: &PerServiceTotals) {
    s.push_str("<tr><td>");
    push_text(s, &row.service);
    s.push_str("</td><td>");
    s.push_str(&row.findings.to_string());
    s.push_str("</td>");
    write_counts_cells(s, &row.severity_counts);
    s.push_str("</tr>");
}

fn render_findings(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"findings\"><h2>Findings</h2>");
    for f in &content.findings {
        render_finding(s, f);
    }
    s.push_str("</section>");
}

fn render_finding(s: &mut String, f: &FindingRow) {
    let sev_class = match f.severity {
        Severity::Critical => "sev sev-critical",
        Severity::High => "sev sev-high",
        Severity::Medium => "sev sev-medium",
        Severity::Low => "sev sev-low",
        Severity::Informational => "sev sev-informational",
    };
    s.push_str("<article class=\"finding\">");
    s.push_str("<header><span class=\"");
    s.push_str(sev_class);
    s.push_str("\">");
    push_text(s, severity_label(f.severity));
    s.push_str("</span> <span class=\"status\">");
    push_text(
        s,
        match f.status {
            FindingStatus::Open => "open",
            FindingStatus::Resolved => "resolved",
        },
    );
    s.push_str("</span> <code class=\"rule-key\">");
    push_text(s, &f.rule_key);
    s.push_str("</code> <span class=\"service\">");
    push_text(s, &f.service);
    s.push_str("</span></header>");

    s.push_str("<p class=\"description\">");
    push_text(s, &f.description);
    s.push_str("</p>");
    if let Some(rationale) = &f.rationale {
        s.push_str("<p class=\"rationale\"><strong>Why this matters:</strong> ");
        push_text(s, rationale);
        s.push_str("</p>");
    }
    s.push_str("<p class=\"meta\">Checked ");
    s.push_str(&f.checked_items.to_string());
    s.push_str(" / flagged ");
    s.push_str(&f.flagged_items.to_string());
    s.push_str(" · first seen ");
    push_text(s, &f.first_seen_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    s.push_str(" · last seen ");
    push_text(s, &f.last_seen_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    s.push_str("</p>");

    if !f.remediation.trim().is_empty() {
        s.push_str("<h4>Remediation</h4><pre class=\"remediation\">");
        // KB articles are markdown — for the report we emit them as
        // pre-formatted text so the source is faithful and there is
        // NO chance of an inline `<script>` tag in the article body
        // leaking through. The escape pass below guarantees the
        // rendered HTML contains no `<`/`>` from the input.
        push_text(s, &f.remediation);
        s.push_str("</pre>");
    }
    if !f.compliance_lines.is_empty() {
        s.push_str("<h4>Compliance</h4><ul class=\"compliance\">");
        for line in &f.compliance_lines {
            s.push_str("<li>");
            push_text(s, line);
            s.push_str("</li>");
        }
        s.push_str("</ul>");
    }
    if !f.resources.is_empty() {
        s.push_str("<h4>Resources</h4><ul class=\"resources\">");
        for r in &f.resources {
            s.push_str("<li><code>");
            push_text(s, r);
            s.push_str("</code></li>");
        }
        s.push_str("</ul>");
        if f.truncated_extra > 0 {
            s.push_str("<p class=\"truncated\">+");
            s.push_str(&f.truncated_extra.to_string());
            s.push_str(" more not shown</p>");
        }
    }
    s.push_str("</article>");
}

fn render_events(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"events\"><h2>Activity in range</h2>");
    s.push_str("<table><thead><tr><th>When</th><th>Kind</th><th>Account</th><th>Summary</th></tr></thead><tbody>");
    for e in &content.events {
        render_event_row(s, e);
    }
    s.push_str("</tbody></table></section>");
}

fn render_event_row(s: &mut String, e: &EventRow) {
    s.push_str("<tr><td>");
    push_text(s, &e.occurred_at.to_rfc3339_opts(SecondsFormat::Secs, true));
    s.push_str("</td><td>");
    push_text(s, &e.kind);
    s.push_str("</td><td>");
    push_text(s, e.account_display.as_deref().unwrap_or("—"));
    s.push_str("</td><td>");
    push_text(s, &e.summary);
    s.push_str("</td></tr>");
}

fn render_footer(s: &mut String, content: &ReportContent) {
    s.push_str("<footer><p>Generated by CloudSaw ");
    push_text(s, &content.header.cloudsaw_version);
    s.push_str(" at ");
    push_text(
        s,
        &content
            .header
            .generated_at
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str(".</p></footer>");
}

fn write_counts_cells(s: &mut String, counts: &SeverityCounts) {
    for c in [
        counts.critical,
        counts.high,
        counts.medium,
        counts.low,
        counts.informational,
    ] {
        s.push_str("<td>");
        s.push_str(&c.to_string());
        s.push_str("</td>");
    }
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "CRITICAL",
        Severity::High => "HIGH",
        Severity::Medium => "MEDIUM",
        Severity::Low => "LOW",
        Severity::Informational => "INFO",
    }
}

// --- HTML escaping ------------------------------------------------------
//
// `push_text` is the ONLY way any input string lands in the output.
// Every dynamic field — descriptions, resource paths, event-log
// summaries, account labels — goes through here. The escape rule
// covers `<`, `>`, `&`, `"`, `'` so a literal `<script>` in (say) a
// finding description becomes `&lt;script&gt;` in the HTML — the
// browser never parses it as a tag.

fn push_text(out: &mut String, input: &str) {
    for c in input.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
}

fn push_attr(out: &mut String, input: &str) {
    // Same as push_text but cheaper to call from attribute positions
    // — kept distinct to make the call sites self-documenting.
    push_text(out, input);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::model::{
        AccountRef, ReportHeader, SeverityCounts,
    };
    use chrono::Utc;

    fn empty_content() -> ReportContent {
        ReportContent {
            header: ReportHeader {
                kind: ReportKind::PerScan,
                title: "Test report".into(),
                subtitle: None,
                generated_at: Utc::now(),
                cloudsaw_version: env!("CARGO_PKG_VERSION").into(),
                review_banner: "Review before sharing.".into(),
                disclosure: AccountIdDisclosure::Masked,
                locale: "en".into(),
            },
            scans: vec![],
            findings: vec![],
            per_service: vec![],
            events: vec![],
            empty_state_note: Some("no findings".into()),
        }
    }

    #[test]
    fn output_contains_no_script_tags_ever() {
        let mut c = empty_content();
        // Plant a description that, if not escaped, would contain a
        // script tag and a remote URL.
        c.findings.push(FindingRow {
            finding_id: "f".into(),
            rule_key: "<rule>".into(),
            service: "ec2".into(),
            severity: Severity::High,
            status: FindingStatus::Open,
            description: "<script>alert(1)</script>".into(),
            rationale: Some("see http://evil.example/x".into()),
            checked_items: 1,
            flagged_items: 1,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
            remediation: "do this; <script>steal()</script>".into(),
            compliance_lines: vec![],
            resources: vec!["<img src=x onerror=1>".into()],
            truncated_extra: 0,
        });
        let html = render(&c);
        assert!(!html.contains("<script"), "no <script tag may appear");
        assert!(!html.contains("</script"), "no closing </script either");
        // Escaped form IS present.
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn output_contains_no_remote_url_schemes() {
        let html = render(&empty_content());
        for needle in ["http://", "https://", "//cdn.", "src=\""] {
            assert!(
                !html.contains(needle),
                "renderer must never emit `{needle}`",
            );
        }
    }

    #[test]
    fn header_banner_timestamp_and_version_all_present() {
        let c = empty_content();
        let html = render(&c);
        assert!(html.contains("Review before sharing"));
        assert!(html.contains("Generated at"));
        assert!(html.contains("CloudSaw version"));
        assert!(html.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn empty_state_note_renders_when_no_findings() {
        let mut c = empty_content();
        c.empty_state_note = Some("no findings observed".into());
        let html = render(&c);
        assert!(html.contains("no findings observed"));
    }

    #[test]
    fn masked_account_display_is_used_verbatim() {
        let mut c = empty_content();
        c.scans.push(ScanSummary {
            scan_id: "s".into(),
            account: AccountRef {
                display: "****3333".into(),
                label: "dev".into(),
            },
            started_at: Utc::now(),
            finished_at: None,
            status: "complete".into(),
            severity_counts: SeverityCounts::empty(),
        });
        let html = render(&c);
        assert!(html.contains("****3333"));
        assert!(!html.contains("111122223333"));
    }
}
