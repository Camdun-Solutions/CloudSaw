// Self-contained HTML renderer (Contract 15 §Constraints).
//
// Output invariants:
//   * One inline `<script>` block is emitted at the end of the body
//     (PR #72), carrying ONLY compile-time-static code from the
//     `JS_PAGINATION_FILTER` constant. The script never receives
//     user input as code: it operates exclusively on DOM properties
//     (`style.display`, `textContent`, `dataset`) and never calls
//     `eval`, `new Function`, `innerHTML=`, `document.write`, or
//     any network-fetch API. Every dynamic value flows in through
//     escaped `data-*` attributes (see `push_attr`). The regression
//     test asserts those exclusions.
//   * No remote URLs. Every URL the renderer ships is either `mailto:`,
//     `#` (anchor), or `data:` (compile-time-embedded bytes — see
//     `LOGO_PNG_BASE64`). The CSS is inlined in a `<style>` block;
//     the only `<img>` carries a `data:image/png;base64,...` src whose
//     bytes come from `icons/128x128.png` at build time and cannot be
//     influenced by any input field. No `<link rel="stylesheet">`,
//     `<iframe>`, `<object>` elements are ever emitted.
//   * No external resource loads. Same fence as above — there is no
//     value of any input that can introduce a network reference,
//     because every text field is HTML-escaped at the boundary, and
//     the inline script never calls `fetch`, `XMLHttpRequest`, or
//     `WebSocket`.
//   * Banner, brand logo, generation timestamp, and CloudSaw version
//     live in the header section so every report carries the
//     mandatory copy + branding.
//
// The renderer is a pure function over `ReportContent`. Tests assert
// the no-eval / no-fetch / no-remote-url / banner-present invariants
// on every shape of report.

use chrono::SecondsFormat;

use super::model::{
    AccountIdDisclosure, EventRow, FindingRow, PerServiceTotals, ReportContent, ReportKind,
    ScanSummary, SeverityCounts,
};
use crate::findings::{FindingStatus, Severity};

const CSS: &str = include_str!("report.css");

// Compile-time-embedded brand logo for the report header. Generated
// by `build.rs::generate_logo_base64()` from `icons/128x128.png`. See
// the comment block at the top of this file for why this is the only
// data: URI the renderer is allowed to emit.
include!(concat!(env!("OUT_DIR"), "/logo_base64.rs"));

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
    // PR #72: inline pagination + filter script. The body is a
    // compile-time constant — see the module doc-comment for the
    // safety envelope. The script attaches to data-* attributes
    // emitted by `render_findings` and `render_events`.
    s.push_str("<script>");
    s.push_str(JS_PAGINATION_FILTER);
    s.push_str("</script>");
    s.push_str("</body></html>");
    s
}

/// PR #72 — client-side pagination + filter script for the
/// findings / activity-log tables. Inlined into every custom
/// report so the saved file works offline.
///
/// Safety surface (audited line by line):
///   * No `eval`, no `new Function`, no `setTimeout`/`setInterval`
///     with string args.
///   * No `innerHTML`, no `document.write`, no `outerHTML`.
///   * No `fetch`, no `XMLHttpRequest`, no `WebSocket`,
///     no `navigator.sendBeacon`.
///   * No `import`/dynamic import.
///   * Reads from element `dataset.*` (already HTML-escaped by the
///     renderer via `push_attr`) and `value` / `textContent` (also
///     either escaped or compile-time constant).
const JS_PAGINATION_FILTER: &str = r#"
(function(){
  function applyAll(tableId){
    var table=document.getElementById(tableId);
    if(!table)return;
    var filters=document.querySelectorAll('[data-filter-target="'+tableId+'"]');
    var rows=table.querySelectorAll('tbody tr');
    var pageSize=parseInt(table.dataset.pageSize,10)||10;
    var shown=0,matched=0,total=rows.length;
    rows.forEach(function(row){
      var ok=true;
      filters.forEach(function(f){
        if(!ok)return;
        var field=f.dataset.filterField||'';
        var value=(f.value||'').toString().toLowerCase().trim();
        if(!value)return;
        if(field==='text'){
          if(row.textContent.toLowerCase().indexOf(value)<0)ok=false;
        } else {
          var rv=(row.dataset[field]||'').toLowerCase();
          if(rv!==value)ok=false;
        }
      });
      if(ok){
        matched++;
        if(shown<pageSize){row.style.display='';shown++;}
        else{row.style.display='none';}
      } else {
        row.style.display='none';
      }
    });
    var info=document.querySelector('[data-pagination-info-for="'+tableId+'"]');
    if(info){info.textContent='Showing '+shown+' of '+matched+' matched ('+total+' total)';}
  }
  document.querySelectorAll('[data-filter-target]').forEach(function(f){
    var ev=(f.tagName==='SELECT')?'change':'input';
    f.addEventListener(ev,function(){applyAll(f.dataset.filterTarget);});
  });
  document.querySelectorAll('[data-page-target]').forEach(function(s){
    s.addEventListener('change',function(){
      var t=document.getElementById(s.dataset.pageTarget);
      if(t){t.dataset.pageSize=s.value;}
      applyAll(s.dataset.pageTarget);
    });
  });
  document.querySelectorAll('table[data-page-size]').forEach(function(t){applyAll(t.id);});
})();
"#;

fn render_header(s: &mut String, content: &ReportContent) {
    s.push_str("<header class=\"report-header\">");
    // The sensitive-data review banner is mandatory on every report.
    s.push_str("<div class=\"banner\" role=\"alert\">");
    push_text(s, &content.header.review_banner);
    s.push_str("</div>");
    // Brand banner — logo + title in a flex row. The logo src is a
    // compile-time-baked data: URI; it cannot be influenced by user
    // input. When LOGO_PNG_BASE64 is empty (dev builds before icons/
    // is populated) the `<img>` renders as a broken-image placeholder
    // which the CSS hides via `img:not([src]), img[src=""]`.
    s.push_str("<div class=\"brand\">");
    if !LOGO_PNG_BASE64.is_empty() {
        s.push_str("<img class=\"brand-logo\" alt=\"\" src=\"data:image/png;base64,");
        s.push_str(LOGO_PNG_BASE64);
        s.push_str("\">");
    }
    s.push_str("<h1>");
    push_text(s, &content.header.title);
    s.push_str("</h1>");
    s.push_str("</div>");
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
            // PR #71: surface the masking PATTERN itself ("****XXXX")
            // so the reader of the report sees the format they'll
            // see in the rows below, not an opaque "masked (default)"
            // label. Mirrors the convention CloudSaw's UI uses in
            // log surfaces and the accounts table.
            AccountIdDisclosure::Masked => "****XXXX (last 4 of each ID only)",
            AccountIdDisclosure::Full => "full 12-digit account IDs (explicit opt-in)",
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
    push_text(
        s,
        &scan.started_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    );
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

    // PR #72: findings now render as a filterable + paginated table
    // (search + severity + service + status + rows-per-page). The
    // per-service grouping the old layout used has moved to the
    // "Findings by service" section above; this view is for digging.
    //
    // Service-option list: prefer `per_service` (ranked by finding
    // count) so the dropdown order mirrors the per-service totals
    // table; fall back to deriving from `findings` (dedup, in first-
    // appearance order) so the dropdown is still populated for
    // callers that build content without pre-tallied per_service
    // (e.g. tests, the activity-only custom report).
    let mut services: Vec<&str> = content
        .per_service
        .iter()
        .map(|p| p.service.as_str())
        .collect();
    if services.is_empty() {
        for f in &content.findings {
            let svc = f.service.as_str();
            if !services.contains(&svc) {
                services.push(svc);
            }
        }
    }

    s.push_str("<div class=\"filter-bar\">");
    s.push_str("<label><span>Search</span><input type=\"search\" placeholder=\"description / rule_key\" data-filter-target=\"findings-table\" data-filter-field=\"text\"></label>");
    s.push_str("<label><span>Severity</span><select data-filter-target=\"findings-table\" data-filter-field=\"severity\">");
    s.push_str("<option value=\"\">All</option>");
    s.push_str("<option value=\"critical\">Critical</option>");
    s.push_str("<option value=\"high\">High</option>");
    s.push_str("<option value=\"medium\">Medium</option>");
    s.push_str("<option value=\"low\">Low</option>");
    s.push_str("<option value=\"informational\">Informational</option>");
    s.push_str("</select></label>");
    s.push_str("<label><span>Service</span><select data-filter-target=\"findings-table\" data-filter-field=\"service\">");
    s.push_str("<option value=\"\">All</option>");
    for svc in &services {
        s.push_str("<option value=\"");
        push_attr(s, svc);
        s.push_str("\">");
        push_text(s, svc);
        s.push_str("</option>");
    }
    s.push_str("</select></label>");
    s.push_str("<label><span>Status</span><select data-filter-target=\"findings-table\" data-filter-field=\"status\">");
    s.push_str("<option value=\"\">All</option>");
    s.push_str("<option value=\"open\">Open</option>");
    s.push_str("<option value=\"resolved\">Resolved</option>");
    s.push_str("</select></label>");
    s.push_str("<label><span>Rows per page</span><select data-page-target=\"findings-table\">");
    s.push_str("<option value=\"10\">10</option>");
    s.push_str("<option value=\"20\">20</option>");
    s.push_str("<option value=\"50\">50</option>");
    s.push_str("<option value=\"100\">100</option>");
    s.push_str("</select></label>");
    s.push_str("</div>");

    s.push_str("<table id=\"findings-table\" data-page-size=\"10\" class=\"data-table\">");
    s.push_str("<thead><tr>");
    s.push_str("<th>Severity</th><th>Service</th><th>Rule</th><th>Status</th><th>Description</th><th>Flagged</th><th>Last seen</th>");
    s.push_str("</tr></thead><tbody>");
    for f in &content.findings {
        render_findings_row(s, f);
    }
    s.push_str("</tbody></table>");
    s.push_str("<p class=\"pagination-info\" data-pagination-info-for=\"findings-table\"></p>");
    s.push_str("</section>");
}

fn render_findings_row(s: &mut String, f: &FindingRow) {
    let severity_token = severity_token(f.severity);
    let status_token = status_token(f.status);
    s.push_str("<tr data-severity=\"");
    push_attr(s, severity_token);
    s.push_str("\" data-service=\"");
    push_attr(s, &f.service);
    s.push_str("\" data-status=\"");
    push_attr(s, status_token);
    s.push_str("\"><td><span class=\"sev sev-");
    push_attr(s, severity_token);
    s.push_str("\">");
    push_text(s, severity_token);
    s.push_str("</span></td><td>");
    push_text(s, &f.service);
    s.push_str("</td><td><code>");
    push_text(s, &f.rule_key);
    s.push_str("</code></td><td>");
    push_text(s, status_token);
    s.push_str("</td><td>");
    push_text(s, &f.description);
    s.push_str("</td><td>");
    s.push_str(&f.flagged_items.to_string());
    s.push_str("</td><td>");
    push_text(
        s,
        &f.last_seen_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str("</td></tr>");
}

fn severity_token(s: Severity) -> &'static str {
    match s {
        Severity::Critical => "critical",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
        Severity::Informational => "informational",
    }
}

fn status_token(s: FindingStatus) -> &'static str {
    match s {
        FindingStatus::Open => "open",
        FindingStatus::Resolved => "resolved",
    }
}

// PR #72: `render_service_group`, `write_summary_pills`,
// `render_finding`, and `render_remediation_block` were the
// per-service-card layout for findings. They're now superseded by
// the filterable/paginated `findings-table` rendered from
// `render_findings_row`. Kept here as `#[allow(dead_code)]` so the
// existing per-scan report code-path (which still uses cards if a
// future contract surfaces them) doesn't suddenly lose the helpers
// — but the custom-report path no longer reaches them.
#[allow(dead_code)]
fn render_service_group(s: &mut String, service: &str, rows: &[&FindingRow]) {
    // Severity tally for the summary line. Critical+High open by
    // default so the user sees what matters without expanding; the
    // rest stay closed for compact viewing.
    let mut counts = SeverityCounts::empty();
    for r in rows {
        counts.bump(r.severity);
    }
    let open_by_default = counts.critical > 0 || counts.high > 0;

    s.push_str("<details class=\"service-group\"");
    if open_by_default {
        s.push_str(" open");
    }
    s.push_str("><summary class=\"service-summary\"><span class=\"service-name\">");
    push_text(s, service);
    s.push_str("</span> <span class=\"service-count\">");
    s.push_str(&rows.len().to_string());
    s.push_str(if rows.len() == 1 {
        " finding"
    } else {
        " findings"
    });
    s.push_str("</span>");
    write_summary_pills(s, &counts);
    s.push_str("</summary>");
    for f in rows {
        render_finding(s, f);
    }
    s.push_str("</details>");
}

#[allow(dead_code)]
fn write_summary_pills(s: &mut String, counts: &SeverityCounts) {
    s.push_str("<span class=\"sev-tally\">");
    for (count, class, label) in [
        (counts.critical, "sev sev-critical", "C"),
        (counts.high, "sev sev-high", "H"),
        (counts.medium, "sev sev-medium", "M"),
        (counts.low, "sev sev-low", "L"),
        (counts.informational, "sev sev-informational", "I"),
    ] {
        if count == 0 {
            continue;
        }
        s.push_str("<span class=\"");
        s.push_str(class);
        s.push_str(" sev-pill\">");
        s.push_str(label);
        s.push(' ');
        s.push_str(&count.to_string());
        s.push_str("</span>");
    }
    s.push_str("</span>");
}

#[allow(dead_code)]
fn render_finding(s: &mut String, f: &FindingRow) {
    let sev_class = match f.severity {
        Severity::Critical => "sev sev-critical",
        Severity::High => "sev sev-high",
        Severity::Medium => "sev sev-medium",
        Severity::Low => "sev sev-low",
        Severity::Informational => "sev sev-informational",
    };
    // PR #56: severity-colored left-border on each finding card. The
    // border-color is driven by a per-severity modifier class on the
    // <article> itself; the CSS handles the actual color. Same
    // vocabulary as the inline `.sev` badge so the card and the badge
    // agree on the color of the severity.
    let card_class = match f.severity {
        Severity::Critical => "finding finding-critical",
        Severity::High => "finding finding-high",
        Severity::Medium => "finding finding-medium",
        Severity::Low => "finding finding-low",
        Severity::Informational => "finding finding-informational",
    };
    s.push_str("<article class=\"");
    s.push_str(card_class);
    s.push_str("\">");
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
    push_text(
        s,
        &f.first_seen_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str(" · last seen ");
    push_text(
        s,
        &f.last_seen_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str("</p>");

    let has_remediation = !f.remediation.trim().is_empty();
    let has_terraform = !f.terraform_fix.trim().is_empty();
    let has_aws_cli = !f.aws_cli_fix.trim().is_empty();
    if has_remediation || has_terraform || has_aws_cli {
        s.push_str("<h4>Remediation</h4>");
        // PR #56: each remediation flavor renders as its own
        // collapsible <details>. We can't use real tabs (no scripts
        // allowed — Contract 15 §Constraints), so the closest fit is
        // separate disclosures the reader can expand independently.
        // The main remediation defaults open, variants default closed.
        if has_remediation {
            render_remediation_block(s, "Overview", &f.remediation, true);
        }
        if has_terraform {
            render_remediation_block(s, "Terraform Fix", &f.terraform_fix, false);
        }
        if has_aws_cli {
            render_remediation_block(s, "AWS CLI Fix", &f.aws_cli_fix, false);
        }
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

#[allow(dead_code)]
fn render_remediation_block(s: &mut String, label: &str, body: &str, open: bool) {
    s.push_str("<details class=\"remediation-block\"");
    if open {
        s.push_str(" open");
    }
    s.push_str("><summary>");
    push_text(s, label);
    // KB articles are markdown — for the report we emit them as
    // pre-formatted text so the source is faithful and there is
    // NO chance of an inline `<script>` tag in the article body
    // leaking through. The escape pass guarantees the rendered HTML
    // contains no `<`/`>` from the input.
    s.push_str("</summary><pre class=\"remediation\">");
    push_text(s, body);
    s.push_str("</pre></details>");
}

fn render_events(s: &mut String, content: &ReportContent) {
    s.push_str("<section class=\"events\"><h2>Activity in range</h2>");

    // PR #72: same filter / pagination pattern as the findings
    // table. Activity rows are searchable by free-text against the
    // summary column, and the kind dropdown is populated from the
    // distinct kinds actually present in the events list (no use
    // dangling every theoretical EventKind).
    let mut kinds: Vec<&str> = content.events.iter().map(|e| e.kind.as_str()).collect();
    kinds.sort();
    kinds.dedup();

    s.push_str("<div class=\"filter-bar\">");
    s.push_str("<label><span>Search</span><input type=\"search\" placeholder=\"summary\" data-filter-target=\"events-table\" data-filter-field=\"text\"></label>");
    s.push_str("<label><span>Kind</span><select data-filter-target=\"events-table\" data-filter-field=\"kind\">");
    s.push_str("<option value=\"\">All</option>");
    for k in &kinds {
        s.push_str("<option value=\"");
        push_attr(s, k);
        s.push_str("\">");
        push_text(s, k);
        s.push_str("</option>");
    }
    s.push_str("</select></label>");
    s.push_str("<label><span>Rows per page</span><select data-page-target=\"events-table\">");
    s.push_str("<option value=\"10\">10</option>");
    s.push_str("<option value=\"20\">20</option>");
    s.push_str("<option value=\"50\">50</option>");
    s.push_str("<option value=\"100\">100</option>");
    s.push_str("</select></label>");
    s.push_str("</div>");

    s.push_str("<table id=\"events-table\" data-page-size=\"10\" class=\"data-table\">");
    s.push_str(
        "<thead><tr><th>When</th><th>Kind</th><th>Account</th><th>Summary</th></tr></thead><tbody>",
    );
    for e in &content.events {
        render_event_row(s, e);
    }
    s.push_str("</tbody></table>");
    s.push_str("<p class=\"pagination-info\" data-pagination-info-for=\"events-table\"></p>");
    s.push_str("</section>");
}

fn render_event_row(s: &mut String, e: &EventRow) {
    s.push_str("<tr data-kind=\"");
    push_attr(s, &e.kind);
    s.push_str("\"><td>");
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
    use crate::reports::model::{AccountRef, ReportHeader, SeverityCounts};
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
    fn user_input_script_tags_are_escaped_and_no_unsafe_js_apis_are_emitted() {
        // PR #72: the renderer DOES emit one inline `<script>` block —
        // the compile-time-static pagination/filter helper. The test
        // now asserts (a) user input that LOOKS like a script tag is
        // escaped (the original XSS-prevention behavior), AND (b)
        // the emitted JS body never reaches for unsafe APIs.
        let mut c = empty_content();
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
            terraform_fix: "resource \"aws_x\" { <script>alert(2)</script> }".into(),
            aws_cli_fix: "aws s3 ls; <script>alert(3)</script>".into(),
            compliance_lines: vec![],
            resources: vec!["<img src=x onerror=1>".into()],
            truncated_extra: 0,
        });
        let html = render(&c);
        // User-typed script tag (and the matching closing tag) must
        // be escaped — the escape form `&lt;script&gt;` carries the
        // original intent without executing.
        assert!(html.contains("&lt;script&gt;"));
        // The renderer never emits more than ONE `<script>` opening
        // tag (the inline pagination helper) and ONE `</script>`
        // closing tag.
        assert_eq!(html.matches("<script>").count(), 1);
        assert_eq!(html.matches("</script>").count(), 1);
        // The inline script must NEVER use code-execution or
        // network-fetch APIs.
        for unsafe_api in [
            " eval(",
            "eval(\"",
            "new Function(",
            "document.write(",
            ".innerHTML=",
            ".outerHTML=",
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "sendBeacon",
            "import(",
        ] {
            assert!(
                !html.contains(unsafe_api),
                "inline pagination script must not reference unsafe API `{unsafe_api}`",
            );
        }
    }

    #[test]
    fn output_contains_no_remote_url_schemes() {
        let html = render(&empty_content());
        for needle in ["http://", "https://", "//cdn."] {
            assert!(
                !html.contains(needle),
                "renderer must never emit `{needle}`",
            );
        }
        // The only HTML attribute that takes a URL the renderer is
        // allowed to emit is the brand logo's `src="data:image/png;..."` —
        // see the renderer's top-of-file docstring. Any OTHER URL-bearing
        // attribute would be a regression. We isolate the BODY (the
        // <style> block is allowed `src=""` inside CSS selectors so we
        // ignore that section).
        let body_start = html.find("</style></head><body>").unwrap_or(0);
        let body = &html[body_start..];
        let stripped = body.replace("src=\"data:image/png;base64,", "");
        assert!(
            !stripped.contains("src=\""),
            "the only `src=` allowed in the body is the brand logo's data: URI",
        );
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

    fn finding_row(rule_key: &str, service: &str, severity: Severity) -> FindingRow {
        FindingRow {
            finding_id: rule_key.into(),
            rule_key: rule_key.into(),
            service: service.into(),
            severity,
            status: FindingStatus::Open,
            description: format!("desc for {rule_key}"),
            rationale: None,
            checked_items: 1,
            flagged_items: 1,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
            remediation: String::new(),
            terraform_fix: String::new(),
            aws_cli_fix: String::new(),
            compliance_lines: vec![],
            resources: vec![],
            truncated_extra: 0,
        }
    }

    // PR #72: the per-service `<details>` cards and per-finding card
    // layout were replaced with a single filterable + paginated
    // `<table id="findings-table" class="data-table">`. The tests
    // below assert the new structure: one `<tr>` per finding, each
    // tagged with `data-severity` / `data-service` / `data-status`
    // attributes (consumed by the inline pagination/filter helper),
    // the severity pill class stays, and the filter bar exposes the
    // expected per-column controls.

    #[test]
    fn findings_render_in_a_single_filterable_paginated_table() {
        let mut c = empty_content();
        c.empty_state_note = None;
        c.findings.push(finding_row("iam-a", "iam", Severity::High));
        c.findings
            .push(finding_row("iam-b", "iam", Severity::Medium));
        c.findings.push(finding_row("s3-a", "s3", Severity::Low));
        let html = render(&c);

        // Exactly one findings table, with pagination wired up.
        assert!(html
            .contains("<table id=\"findings-table\" data-page-size=\"10\" class=\"data-table\">"));
        // Three rows, each tagged with the filter dimensions the
        // inline helper reads. We do not require any specific cell
        // count here; the helper keys off these data attributes.
        assert_eq!(
            html.matches("<tr data-severity=\"high\" data-service=\"iam\"")
                .count(),
            1
        );
        assert_eq!(
            html.matches("<tr data-severity=\"medium\" data-service=\"iam\"")
                .count(),
            1
        );
        assert_eq!(
            html.matches("<tr data-severity=\"low\" data-service=\"s3\"")
                .count(),
            1
        );
        // Both services populate the Service filter <select> exactly
        // once (deduped from the findings list).
        assert_eq!(
            html.matches("<option value=\"iam\">iam</option>").count(),
            1
        );
        assert_eq!(html.matches("<option value=\"s3\">s3</option>").count(), 1);
        // The filter bar carries the four documented controls.
        assert!(html.contains("data-filter-field=\"text\""));
        assert!(html.contains("data-filter-field=\"severity\""));
        assert!(html.contains("data-filter-field=\"service\""));
        assert!(html.contains("data-filter-field=\"status\""));
        assert!(html.contains("data-page-target=\"findings-table\""));
    }

    #[test]
    fn finding_row_severity_pill_carries_severity_class() {
        let mut c = empty_content();
        c.empty_state_note = None;
        c.findings
            .push(finding_row("ec2-x", "ec2", Severity::Critical));
        c.findings.push(finding_row("ec2-y", "ec2", Severity::Low));
        let html = render(&c);
        // The severity pill inside each <tr> uses the same color
        // vocabulary the rest of the app does. The card-level
        // .finding-{sev} left border class is gone — superseded by
        // the table layout — but the in-cell pill stays.
        assert!(html.contains("<span class=\"sev sev-critical\">"));
        assert!(html.contains("<span class=\"sev sev-low\">"));
    }

    #[test]
    fn findings_default_to_ten_rows_with_pagination_options() {
        let mut c = empty_content();
        c.empty_state_note = None;
        c.findings.push(finding_row("iam-a", "iam", Severity::High));
        let html = render(&c);
        // 10 is the default; 10/20/50/100 are the four offered sizes.
        assert!(html.contains("data-page-size=\"10\""));
        for &size in &["10", "20", "50", "100"] {
            let opt = format!("<option value=\"{size}\"");
            assert!(
                html.contains(&opt),
                "page-size option {size} missing from rows-per-page select"
            );
        }
        // A live pagination info <p> is emitted (the helper fills it
        // with "Showing X–Y of Z" at render-time).
        assert!(html.contains("data-pagination-info-for=\"findings-table\""));
    }
}
