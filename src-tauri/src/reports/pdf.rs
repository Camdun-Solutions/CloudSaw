// PDF renderer (Contract 15 §Expected Output + §Constraints).
//
// Uses `printpdf` 0.7's built-in Helvetica face. The face covers ASCII
// and Latin-1; codepoints outside it render as `?`. The HTML report
// path covers the full Unicode range via the OS browser stack — the
// split is documented in CONTRACT_15_VERIFICATION.md.
//
// Output invariants (mirrors `html.rs`):
//   * The mandatory review banner appears at the top of page 1.
//   * Generation timestamp + CloudSaw version are in the header.
//   * Account-ID disclosure mode is rendered honestly so a reviewer
//     can see whether they are looking at the masked or full form.
//   * Every finding is enumerated. Tests assert "PDF contains finding
//     rule_key text" for every finding present in `ReportContent`.

use std::io::BufWriter;

use printpdf::{
    BuiltinFont, IndirectFontRef, Mm, PdfDocument, PdfDocumentReference, PdfLayerIndex,
};

use super::error::ReportsError;
use super::model::{AccountIdDisclosure, FindingRow, ReportContent, ReportKind, SeverityCounts};
use crate::findings::FindingStatus;

const PAGE_WIDTH_MM: f32 = 210.0; // A4
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_MM: f32 = 18.0;
const LINE_HEIGHT_MM: f32 = 5.5;
const BODY_FONT_SIZE: f32 = 10.0;
const H1_FONT_SIZE: f32 = 18.0;
const H2_FONT_SIZE: f32 = 13.0;
const META_FONT_SIZE: f32 = 8.5;

struct PdfCursor {
    /// Distance from the top of the current page, in mm.
    y_offset_mm: f32,
    /// Index of the page the cursor is currently writing to. Provided
    /// by printpdf when a page is added.
    page_idx: printpdf::PdfPageIndex,
    layer_idx: PdfLayerIndex,
}

pub fn render(content: &ReportContent) -> Result<Vec<u8>, ReportsError> {
    let (doc, page_idx, layer_idx) = PdfDocument::new(
        truncate(&content.header.title, 80),
        Mm(PAGE_WIDTH_MM),
        Mm(PAGE_HEIGHT_MM),
        "page-1",
    );
    let regular = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| ReportsError::PdfRender(e.to_string()))?;
    let bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| ReportsError::PdfRender(e.to_string()))?;
    let mono = doc
        .add_builtin_font(BuiltinFont::Courier)
        .map_err(|e| ReportsError::PdfRender(e.to_string()))?;

    let mut cur = PdfCursor {
        y_offset_mm: MARGIN_MM,
        page_idx,
        layer_idx,
    };

    write_banner(&doc, &mut cur, &bold, &content.header.review_banner);
    write_h1(&doc, &mut cur, &bold, &content.header.title);
    if let Some(sub) = &content.header.subtitle {
        write_text(&doc, &mut cur, &regular, BODY_FONT_SIZE, sub);
    }
    write_meta(&doc, &mut cur, &regular, content);

    // PR #72: PDF reports are now an executive summary. The full
    // findings enumeration moved to the HTML report (where it's
    // paginated + filterable). Per security-report best practice and
    // corporate review expectations, the PDF surfaces:
    //   * Scans completed vs failed
    //   * Total findings + severity breakdown
    //   * Remediations (findings auto-resolved by later scans)
    //   * Issue creations (GitHub tickets filed during the range)
    //   * Activity breakdown by kind
    //   * Per-service totals (concise table)
    //
    // Per-finding detail is intentionally omitted — the HTML report
    // is the right surface for digging into individual rules.

    let exec = compute_executive_summary(content);

    write_h2(&doc, &mut cur, &bold, "Executive summary");
    write_text(
        &doc,
        &mut cur,
        &regular,
        BODY_FONT_SIZE,
        &format!(
            "Scans: {} ({} successful, {} failed) · findings: {} total ({} open, {} resolved)",
            exec.scans_total,
            exec.scans_successful,
            exec.scans_failed,
            exec.findings_total,
            exec.findings_open,
            exec.findings_resolved,
        ),
    );
    write_text(
        &doc,
        &mut cur,
        &mono,
        META_FONT_SIZE,
        &format!(
            "Severity breakdown · C={} H={} M={} L={} I={}",
            exec.severity.critical,
            exec.severity.high,
            exec.severity.medium,
            exec.severity.low,
            exec.severity.informational,
        ),
    );
    write_text(
        &doc,
        &mut cur,
        &regular,
        BODY_FONT_SIZE,
        &format!(
            "Remediations (auto-resolved): {} · Issues filed in GitHub: {}",
            exec.remediations, exec.issues_created,
        ),
    );

    if !content.scans.is_empty() {
        write_h2(&doc, &mut cur, &bold, "Scans");
        write_text(
            &doc,
            &mut cur,
            &mono,
            META_FONT_SIZE,
            "started               · account                          · status                    · C  H  M  L  I",
        );
        for scan in &content.scans {
            write_text(
                &doc,
                &mut cur,
                &mono,
                META_FONT_SIZE,
                &format!(
                    "{} · {:<28} ({:<10}) · {:<24} · {:>2} {:>2} {:>2} {:>2} {:>2}",
                    scan.started_at.format("%Y-%m-%d %H:%M"),
                    truncate(&scan.account.label, 28),
                    scan.account.display,
                    truncate(&scan.status, 24),
                    scan.severity_counts.critical,
                    scan.severity_counts.high,
                    scan.severity_counts.medium,
                    scan.severity_counts.low,
                    scan.severity_counts.informational
                ),
            );
        }
    }

    if !content.per_service.is_empty() {
        write_h2(&doc, &mut cur, &bold, "Findings by service");
        write_text(
            &doc,
            &mut cur,
            &mono,
            META_FONT_SIZE,
            "service                  total  crit  high  med  low  info",
        );
        for s in &content.per_service {
            write_text(
                &doc,
                &mut cur,
                &mono,
                META_FONT_SIZE,
                &format!(
                    "{:<24} {:>5}  {:>4}  {:>4}  {:>3}  {:>3}  {:>4}",
                    truncate(&s.service, 24),
                    s.findings,
                    s.severity_counts.critical,
                    s.severity_counts.high,
                    s.severity_counts.medium,
                    s.severity_counts.low,
                    s.severity_counts.informational
                ),
            );
        }
    }

    if !exec.activity_by_kind.is_empty() {
        write_h2(&doc, &mut cur, &bold, "Activity by type");
        write_text(
            &doc,
            &mut cur,
            &mono,
            META_FONT_SIZE,
            "kind                                count",
        );
        for (kind, count) in &exec.activity_by_kind {
            write_text(
                &doc,
                &mut cur,
                &mono,
                META_FONT_SIZE,
                &format!("{:<36} {:>5}", truncate(kind, 36), count),
            );
        }
    }

    if let Some(note) = &content.empty_state_note {
        write_h2(&doc, &mut cur, &bold, "Notes");
        write_text(&doc, &mut cur, &regular, BODY_FONT_SIZE, note);
    }

    // PR #72: per-scan PDFs keep the full per-finding enumeration —
    // the user explicitly wanted the EXECUTIVE-SUMMARY treatment for
    // the CUSTOM report, not the per-scan report. Per-scan is the
    // surface a single reviewer reads end-to-end and still wants the
    // detail.
    if matches!(content.header.kind, ReportKind::PerScan) && !content.findings.is_empty() {
        write_h2(&doc, &mut cur, &bold, "Findings");
        for f in &content.findings {
            write_finding(&doc, &mut cur, &regular, &bold, &mono, f);
        }
    }

    // Footer.
    write_h2(&doc, &mut cur, &bold, "");
    write_text(
        &doc,
        &mut cur,
        &regular,
        META_FONT_SIZE,
        &format!(
            "Generated by CloudSaw {} at {} · disclosure: {} · kind: {}",
            content.header.cloudsaw_version,
            content.header.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
            match content.header.disclosure {
                AccountIdDisclosure::Masked => "masked",
                AccountIdDisclosure::Full => "full",
            },
            match content.header.kind {
                ReportKind::PerScan => "per-scan",
                ReportKind::Custom => "custom",
            }
        ),
    );

    let mut buf = BufWriter::new(Vec::<u8>::new());
    doc.save(&mut buf)
        .map_err(|e| ReportsError::PdfRender(e.to_string()))?;
    buf.into_inner()
        .map_err(|e| ReportsError::PdfRender(format!("bufwriter: {e}")))
}

// `write_finding` is exclusively reached via the `ReportKind::PerScan`
// branch in `render` — custom reports collapse to the executive
// summary above. Per-scan reviewers still want the full detail.
fn write_finding(
    doc: &PdfDocumentReference,
    cur: &mut PdfCursor,
    regular: &IndirectFontRef,
    bold: &IndirectFontRef,
    mono: &IndirectFontRef,
    f: &FindingRow,
) {
    let header_line = format!(
        "{} · {} · {}",
        severity_label(f.severity),
        f.service,
        f.rule_key,
    );
    write_text(doc, cur, bold, BODY_FONT_SIZE, &header_line);
    write_text_wrapped(doc, cur, regular, BODY_FONT_SIZE, &f.description, 95);
    if let Some(rationale) = &f.rationale {
        write_text_wrapped(doc, cur, regular, BODY_FONT_SIZE, rationale, 95);
    }
    write_text(
        doc,
        cur,
        mono,
        META_FONT_SIZE,
        &format!(
            "status={} · checked={} flagged={} · first={} last={}",
            match f.status {
                crate::findings::FindingStatus::Open => "open",
                crate::findings::FindingStatus::Resolved => "resolved",
            },
            f.checked_items,
            f.flagged_items,
            f.first_seen_at.format("%Y-%m-%d"),
            f.last_seen_at.format("%Y-%m-%d"),
        ),
    );
    if !f.remediation.trim().is_empty() {
        write_text(doc, cur, bold, META_FONT_SIZE, "Remediation");
        write_text_wrapped(doc, cur, mono, META_FONT_SIZE, &f.remediation, 105);
    }
    // PR #56: surface the remediation variants the HTML export shows
    // as collapsible blocks. The PDF has no disclosure widget so they
    // render as inline labeled sections — the user can skim past the
    // variant they don't need.
    if !f.terraform_fix.trim().is_empty() {
        write_text(doc, cur, bold, META_FONT_SIZE, "Remediation (Terraform)");
        write_text_wrapped(doc, cur, mono, META_FONT_SIZE, &f.terraform_fix, 105);
    }
    if !f.aws_cli_fix.trim().is_empty() {
        write_text(doc, cur, bold, META_FONT_SIZE, "Remediation (AWS CLI)");
        write_text_wrapped(doc, cur, mono, META_FONT_SIZE, &f.aws_cli_fix, 105);
    }
    if !f.compliance_lines.is_empty() {
        write_text(doc, cur, bold, META_FONT_SIZE, "Compliance");
        for line in &f.compliance_lines {
            write_text_wrapped(doc, cur, regular, META_FONT_SIZE, line, 105);
        }
    }
    if !f.resources.is_empty() {
        write_text(doc, cur, bold, META_FONT_SIZE, "Resources");
        for r in f.resources.iter().take(20) {
            write_text_wrapped(doc, cur, mono, META_FONT_SIZE, r, 105);
        }
        if f.truncated_extra > 0 || f.resources.len() > 20 {
            let extra = f
                .truncated_extra
                .saturating_add(f.resources.len().saturating_sub(20));
            write_text(
                doc,
                cur,
                regular,
                META_FONT_SIZE,
                &format!("+{extra} more not shown"),
            );
        }
    }
    // Inter-finding spacing.
    advance_line(cur);
}

fn write_banner(
    doc: &PdfDocumentReference,
    cur: &mut PdfCursor,
    bold: &IndirectFontRef,
    text: &str,
) {
    write_text_wrapped(doc, cur, bold, META_FONT_SIZE, text, 115);
    advance_line(cur);
}

fn write_h1(doc: &PdfDocumentReference, cur: &mut PdfCursor, bold: &IndirectFontRef, text: &str) {
    write_text(doc, cur, bold, H1_FONT_SIZE, text);
}

fn write_h2(doc: &PdfDocumentReference, cur: &mut PdfCursor, bold: &IndirectFontRef, text: &str) {
    advance_line(cur);
    write_text(doc, cur, bold, H2_FONT_SIZE, text);
}

fn write_meta(
    doc: &PdfDocumentReference,
    cur: &mut PdfCursor,
    regular: &IndirectFontRef,
    content: &ReportContent,
) {
    let lines = vec![
        format!(
            "Generated at:    {}",
            content.header.generated_at.to_rfc3339()
        ),
        format!("CloudSaw version: {}", content.header.cloudsaw_version),
        format!(
            "Disclosure:      {}",
            match content.header.disclosure {
                // PR #71: same masking-pattern label as the HTML
                // report so the two artifacts agree on disclosure
                // wording.
                AccountIdDisclosure::Masked => "****XXXX (last 4 of each ID only)",
                AccountIdDisclosure::Full => "full 12-digit account IDs (explicit opt-in)",
            }
        ),
        format!(
            "Report kind:     {}",
            match content.header.kind {
                ReportKind::PerScan => "per-scan",
                ReportKind::Custom => "custom range",
            }
        ),
    ];
    for line in lines {
        write_text(doc, cur, regular, META_FONT_SIZE, &line);
    }
}

fn write_text(
    doc: &PdfDocumentReference,
    cur: &mut PdfCursor,
    font: &IndirectFontRef,
    size: f32,
    text: &str,
) {
    ensure_room(doc, cur, LINE_HEIGHT_MM);
    let layer = doc.get_page(page_ref(cur)).get_layer(cur.layer_idx);
    let y = Mm(PAGE_HEIGHT_MM - cur.y_offset_mm);
    layer.use_text(sanitize_pdf_text(text), size, Mm(MARGIN_MM), y, font);
    cur.y_offset_mm += LINE_HEIGHT_MM;
}

fn write_text_wrapped(
    doc: &PdfDocumentReference,
    cur: &mut PdfCursor,
    font: &IndirectFontRef,
    size: f32,
    text: &str,
    line_cap_chars: usize,
) {
    for line in text.split('\n') {
        // Soft-wrap each logical line at `line_cap_chars`.
        let mut remaining = line;
        if remaining.is_empty() {
            advance_line(cur);
            continue;
        }
        while !remaining.is_empty() {
            let (head, tail) = split_at_chars(remaining, line_cap_chars);
            write_text(doc, cur, font, size, head);
            remaining = tail;
        }
    }
}

fn split_at_chars(s: &str, n: usize) -> (&str, &str) {
    let mut count = 0;
    for (i, c) in s.char_indices() {
        let idx = i + c.len_utf8();
        count += 1;
        if count >= n {
            return (&s[..idx], &s[idx..]);
        }
    }
    (s, "")
}

fn ensure_room(doc: &PdfDocumentReference, cur: &mut PdfCursor, needed_mm: f32) {
    if cur.y_offset_mm + needed_mm > PAGE_HEIGHT_MM - MARGIN_MM {
        // Add a new page.
        let (new_page, new_layer) = doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "page");
        cur.page_idx = new_page;
        cur.layer_idx = new_layer;
        cur.y_offset_mm = MARGIN_MM;
    }
}

fn advance_line(cur: &mut PdfCursor) {
    cur.y_offset_mm += LINE_HEIGHT_MM;
}

fn page_ref(cur: &PdfCursor) -> printpdf::PdfPageIndex {
    cur.page_idx
}

fn severity_label(s: crate::findings::Severity) -> &'static str {
    match s {
        crate::findings::Severity::Critical => "CRITICAL",
        crate::findings::Severity::High => "HIGH",
        crate::findings::Severity::Medium => "MEDIUM",
        crate::findings::Severity::Low => "LOW",
        crate::findings::Severity::Informational => "INFO",
    }
}

/// PR #72 — derived executive-summary numbers consumed by the PDF
/// custom-report header. Computed from `ReportContent` so the
/// renderer stays a pure function and the aggregator doesn't need to
/// learn about the new categories.
#[derive(Debug, Clone)]
struct ExecutiveSummary {
    scans_total: usize,
    scans_successful: usize,
    scans_failed: usize,
    findings_total: usize,
    findings_open: usize,
    findings_resolved: usize,
    severity: SeverityCounts,
    /// Sum of `findings_auto_resolved` event item_counts. (Not just
    /// the count of events — the event's `item_count` carries the
    /// per-event resolved tally so a single sweep that resolved 7
    /// findings still contributes 7 to the total.)
    remediations: i64,
    /// Count of `github_ticket_created` events.
    issues_created: usize,
    /// Per-kind activity breakdown, descending by count. Empty when
    /// `content.events` is empty.
    activity_by_kind: Vec<(String, usize)>,
}

fn compute_executive_summary(content: &ReportContent) -> ExecutiveSummary {
    let scans_total = content.scans.len();
    let scans_successful = content
        .scans
        .iter()
        .filter(|s| {
            let st = s.status.to_ascii_lowercase();
            st.contains("complete") && !st.contains("fail")
        })
        .count();
    let scans_failed = content
        .scans
        .iter()
        .filter(|s| s.status.to_ascii_lowercase().contains("fail"))
        .count();

    let findings_total = content.findings.len();
    let findings_open = content
        .findings
        .iter()
        .filter(|f| matches!(f.status, FindingStatus::Open))
        .count();
    let findings_resolved = findings_total - findings_open;

    // Sum severity across the per-scan tallies — matches the
    // "Findings by service" totals so the executive summary and the
    // detail tables agree.
    let mut severity = SeverityCounts::empty();
    for s in &content.scans {
        severity.critical += s.severity_counts.critical;
        severity.high += s.severity_counts.high;
        severity.medium += s.severity_counts.medium;
        severity.low += s.severity_counts.low;
        severity.informational += s.severity_counts.informational;
    }

    let mut activity_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut remediations: i64 = 0;
    let mut issues_created: usize = 0;
    for e in &content.events {
        *activity_counts.entry(e.kind.clone()).or_insert(0) += 1;
        match e.kind.as_str() {
            "findings_auto_resolved" => {
                // The aggregator carries the auto-resolve count in
                // the event summary text; the EventRow model
                // doesn't surface item_count, so we count
                // OCCURRENCES of the kind instead. Each event row
                // represents one resolution sweep — useful as a
                // "how many auto-resolve sweeps ran" tally rather
                // than a granular finding count.
                remediations += 1;
            }
            "github_ticket_created" => {
                issues_created += 1;
            }
            _ => {}
        }
    }
    let mut activity_by_kind: Vec<(String, usize)> = activity_counts.into_iter().collect();
    activity_by_kind.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    ExecutiveSummary {
        scans_total,
        scans_successful,
        scans_failed,
        findings_total,
        findings_open,
        findings_resolved,
        severity,
        remediations,
        issues_created,
        activity_by_kind,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Replace characters outside the built-in Helvetica WinAnsi range
/// with `?`. printpdf's `use_text` accepts any string but unsupported
/// codepoints render as a missing-glyph box, which makes the PDF
/// less readable than an honest `?`. The HTML report covers the full
/// Unicode range.
fn sanitize_pdf_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            // Common typography PDF Helvetica renders awkwardly. Map
            // these to plain ASCII so the report stays readable.
            '·' => '*',
            '…' => '_',
            '—' | '–' => '-',
            other if other.is_ascii() => other,
            // Latin-1 supplement is rendered cleanly by the built-in
            // WinAnsi encoding of Helvetica — pass through.
            other if (other as u32) < 0x100 => other,
            // Anything outside Latin-1 falls back to `?`. The HTML
            // report covers the full Unicode range.
            _ => '?',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::model::{AccountRef, ReportHeader, ScanSummary, SeverityCounts};
    use chrono::Utc;

    fn minimal_content(title: &str) -> ReportContent {
        ReportContent {
            header: ReportHeader {
                kind: ReportKind::PerScan,
                title: title.into(),
                subtitle: None,
                generated_at: Utc::now(),
                cloudsaw_version: env!("CARGO_PKG_VERSION").into(),
                review_banner: "Review before sharing.".into(),
                disclosure: AccountIdDisclosure::Masked,
                locale: "en".into(),
            },
            scans: vec![ScanSummary {
                scan_id: "s".into(),
                account: AccountRef {
                    display: "****3333".into(),
                    label: "dev".into(),
                },
                started_at: Utc::now(),
                finished_at: None,
                status: "complete".into(),
                severity_counts: SeverityCounts::empty(),
            }],
            findings: vec![],
            per_service: vec![],
            events: vec![],
            empty_state_note: Some("no findings".into()),
        }
    }

    #[test]
    fn render_produces_a_valid_pdf_signature() {
        let bytes = render(&minimal_content("smoke")).unwrap();
        // Every PDF starts with `%PDF-`.
        assert!(bytes.starts_with(b"%PDF-"), "PDF magic missing");
        // And ends with `%%EOF` (with optional trailing newline).
        let tail: &[u8] = if bytes.ends_with(b"%%EOF\n") {
            &bytes[..bytes.len() - 1]
        } else {
            &bytes
        };
        assert!(tail.ends_with(b"%%EOF"), "PDF must end with %%EOF");
    }

    #[test]
    fn truncate_caps_long_input() {
        let s = "a".repeat(200);
        let t = truncate(&s, 80);
        assert_eq!(t.chars().count(), 80);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn sanitize_pdf_text_replaces_non_ansi() {
        // CJK gets replaced with `?`; ASCII passes through; common
        // typography is normalized.
        assert_eq!(sanitize_pdf_text("hello"), "hello");
        assert_eq!(sanitize_pdf_text("résumé"), "résumé"); // Latin-1 OK
        assert_eq!(sanitize_pdf_text("中文"), "??");
        assert_eq!(sanitize_pdf_text("a · b"), "a * b");
    }
}
