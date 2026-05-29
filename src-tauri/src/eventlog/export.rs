// PR #70 — Activity-log export pipeline.
//
// Three formats:
//   * HTML  — self-contained, themed to match the site (CloudSaw
//             red on saw-black/beige), inline CSS, logo + app name
//             in the header and again in the footer alongside the
//             Apache-2.0 license string.
//   * PDF   — A4 portrait via `printpdf`. Same headerless / footerless
//             body shape as the reports module, but with eventlog-
//             specific columns. Built-in Helvetica typesetting
//             (Latin-1 only — same caveat as the reports PDF).
//   * Excel — Single worksheet "Activity Log" via `rust_xlsxwriter`.
//             First row is a themed header banner with the brand
//             color; data rows use auto-width-ish column sizing.
//
// The IPC commands call into these renderers, write to the
// frontend-supplied path, record an `Export` event, and return an
// `ExportOutcome` to the UI.

use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use super::error::EventLogError;
use super::types::{EventInput, EventKind, EventLogEntry, EventLogFilter};
use super::{list_events, record_event};

/// Mirrors the existing `reports::model::ExportOutcome` shape so the
/// frontend can share the same render path for the success body. The
/// auto_export_* fields don't apply here (activity-log exports don't
/// participate in the auto-export-folder pipeline that scan reports
/// use), but they're surfaced as None/false for consistency.
#[derive(Debug, Clone, Serialize)]
pub struct EventLogExportOutcome {
    pub primary_path: String,
    pub bytes_written: u64,
    pub format: &'static str,
    pub rows_exported: usize,
}

/// Three supported output formats. The IPC layer surfaces them as
/// three distinct commands so the frontend stays type-safe; this enum
/// is the internal pivot.
#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    Html,
    Pdf,
    Xlsx,
}

impl ExportFormat {
    fn label(self) -> &'static str {
        match self {
            ExportFormat::Html => "html",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Xlsx => "xlsx",
        }
    }
}

const APP_LICENSE: &str = "Apache-2.0";

// Reuse the same compile-time-embedded logo the scan-report HTML uses.
include!(concat!(env!("OUT_DIR"), "/logo_base64.rs"));

/// Run an activity-log export end-to-end: pull entries with the
/// requested filter, render to the requested format, write to disk,
/// emit an Export event-log row, and return an outcome to the UI.
pub fn export(
    format: ExportFormat,
    filter: EventLogFilter,
    output_path: &Path,
) -> Result<EventLogExportOutcome, EventLogError> {
    // Force include_cleared = true; an export should always include
    // every row, regardless of whether the user clicked "Clear view".
    let mut filt = filter;
    filt.include_cleared = true;
    let entries = list_events(filt)?;
    let bytes = match format {
        ExportFormat::Html => render_html(&entries).into_bytes(),
        ExportFormat::Pdf => render_pdf(&entries)?,
        ExportFormat::Xlsx => render_xlsx(&entries)?,
    };
    std::fs::write(output_path, &bytes).map_err(|e| EventLogError::Io(e.to_string()))?;
    let bytes_written = bytes.len() as u64;
    let rows_exported = entries.len();

    // Record the export in the activity log itself.
    record_event(
        EventInput::new(
            EventKind::Export,
            format!(
                "Activity log exported ({}, {} entr{}).",
                format.label(),
                rows_exported,
                if rows_exported == 1 { "y" } else { "ies" },
            ),
        )
        .with_path(output_path.to_string_lossy().to_string())
        .with_item_count(rows_exported as i64),
    );

    Ok(EventLogExportOutcome {
        primary_path: output_path.to_string_lossy().to_string(),
        bytes_written,
        format: format.label(),
        rows_exported,
    })
}

// --- HTML ------------------------------------------------------------------

fn render_html(entries: &[EventLogEntry]) -> String {
    let mut s = String::with_capacity(16 * 1024);
    s.push_str("<!DOCTYPE html>\n<html lang=\"en\"><head>");
    s.push_str("<meta charset=\"utf-8\">");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    s.push_str("<meta name=\"referrer\" content=\"no-referrer\">");
    s.push_str("<title>CloudSaw — Activity log</title>");
    s.push_str("<style>");
    s.push_str(HTML_CSS);
    s.push_str("</style></head><body>");

    // Header: logo + app name on a saw-red banner.
    s.push_str("<header class=\"brand\">");
    s.push_str("<div class=\"brand-inner\">");
    if !LOGO_PNG_BASE64.is_empty() {
        s.push_str("<img class=\"brand-logo\" alt=\"\" src=\"data:image/png;base64,");
        s.push_str(LOGO_PNG_BASE64);
        s.push_str("\">");
    }
    s.push_str("<div class=\"brand-text\">");
    s.push_str("<h1>CloudSaw</h1>");
    s.push_str("<p class=\"subtitle\">Activity log export</p>");
    s.push_str("</div>");
    s.push_str("</div>");
    s.push_str("<div class=\"meta\">");
    s.push_str("<p>Generated at <strong>");
    push_text(
        &mut s,
        &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    );
    s.push_str("</strong></p>");
    s.push_str(&format!(
        "<p>{} entr{} included</p>",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" }
    ));
    s.push_str("</div>");
    s.push_str("</header>");

    s.push_str("<main>");
    if entries.is_empty() {
        s.push_str("<p class=\"empty\">No entries matched the export filter.</p>");
    } else {
        s.push_str("<table>");
        s.push_str(
            "<thead><tr><th>When (UTC)</th><th>Kind</th><th>Summary</th><th>Account</th><th>Count</th></tr></thead>",
        );
        s.push_str("<tbody>");
        for e in entries {
            s.push_str("<tr>");
            s.push_str("<td class=\"when\">");
            push_text(
                &mut s,
                &e.occurred_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            );
            s.push_str("</td>");
            s.push_str("<td class=\"kind\">");
            push_text(&mut s, e.kind.as_str());
            s.push_str("</td>");
            s.push_str("<td>");
            push_text(&mut s, &e.summary);
            if let Some(d) = &e.detail {
                s.push_str("<br><span class=\"detail\">");
                push_text(&mut s, d);
                s.push_str("</span>");
            }
            s.push_str("</td>");
            s.push_str("<td class=\"acct\">");
            push_text(&mut s, e.aws_account_id_masked.as_deref().unwrap_or("—"));
            s.push_str("</td>");
            s.push_str("<td class=\"count\">");
            s.push_str(
                &e.item_count
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "—".to_string()),
            );
            s.push_str("</td>");
            s.push_str("</tr>");
        }
        s.push_str("</tbody></table>");
    }
    s.push_str("</main>");

    s.push_str("<footer class=\"brand-footer\">");
    s.push_str("<div class=\"brand-inner\">");
    if !LOGO_PNG_BASE64.is_empty() {
        s.push_str("<img class=\"brand-logo brand-logo-sm\" alt=\"\" src=\"data:image/png;base64,");
        s.push_str(LOGO_PNG_BASE64);
        s.push_str("\">");
    }
    s.push_str("<span class=\"brand-name\">CloudSaw</span>");
    s.push_str("<span class=\"license\">License: ");
    s.push_str(APP_LICENSE);
    s.push_str("</span>");
    s.push_str("</div>");
    s.push_str("</footer>");

    s.push_str("</body></html>");
    s
}

/// Escape the bare minimum HTML special characters so any
/// user-controlled text in the log can safely render inside the
/// generated document.
fn push_text(s: &mut String, raw: &str) {
    for ch in raw.chars() {
        match ch {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&#39;"),
            _ => s.push(ch),
        }
    }
}

const HTML_CSS: &str = r#"
body { margin: 0; font-family: 'Inter', system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif; color: #0A0B0D; background: #F7F8FA; }
header.brand, footer.brand-footer { background: #D52836; color: #FFFFFF; padding: 16px 24px; }
header.brand .brand-inner { display: flex; align-items: center; gap: 14px; max-width: 1100px; margin: 0 auto; }
.brand-logo { width: 44px; height: 44px; border-radius: 6px; background: #fff; padding: 4px; }
.brand-logo-sm { width: 24px; height: 24px; padding: 2px; }
.brand-text h1 { margin: 0; font-size: 1.4rem; letter-spacing: -0.01em; }
.brand-text .subtitle { margin: 2px 0 0; font-size: 0.875rem; opacity: 0.85; }
header.brand .meta { max-width: 1100px; margin: 8px auto 0; font-size: 0.8125rem; opacity: 0.9; }
header.brand .meta p { margin: 2px 0; }
main { max-width: 1100px; margin: 24px auto; padding: 0 24px 32px; }
p.empty { padding: 16px; background: #FFFFFF; border-radius: 10px; color: #4C525C; }
table { width: 100%; border-collapse: collapse; background: #FFFFFF; border-radius: 10px; overflow: hidden; box-shadow: 0 1px 2px rgba(0,0,0,0.04); }
thead { background: #EDEFF3; color: #363B43; }
th, td { text-align: left; padding: 10px 12px; font-size: 0.875rem; vertical-align: top; border-bottom: 1px solid #EDEFF3; }
tbody tr:last-child td { border-bottom: 0; }
.when, .kind, .acct, .count { font-family: 'JetBrains Mono', ui-monospace, SFMono-Regular, monospace; color: #363B43; white-space: nowrap; }
.count { text-align: right; }
.detail { color: #4C525C; font-size: 0.8125rem; }
footer.brand-footer { margin-top: 24px; padding: 12px 24px; font-size: 0.8125rem; }
footer.brand-footer .brand-inner { display: flex; align-items: center; gap: 12px; max-width: 1100px; margin: 0 auto; }
.brand-name { font-weight: 600; }
.license { margin-left: auto; opacity: 0.85; }
"#;

// --- PDF -------------------------------------------------------------------

fn render_pdf(entries: &[EventLogEntry]) -> Result<Vec<u8>, EventLogError> {
    use printpdf::{BuiltinFont, Mm, PdfDocument};

    // A4 portrait.
    const PAGE_W: f32 = 210.0;
    const PAGE_H: f32 = 297.0;
    const MARGIN: f32 = 18.0;
    const LINE_H: f32 = 5.0;
    const BODY_FONT_SIZE: f32 = 9.0;
    const META_FONT_SIZE: f32 = 8.0;

    let (doc, mut page_idx, mut layer_idx) =
        PdfDocument::new("CloudSaw Activity Log", Mm(PAGE_W), Mm(PAGE_H), "page-1");
    let regular = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| EventLogError::Other(format!("pdf font: {e}")))?;
    let bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| EventLogError::Other(format!("pdf font: {e}")))?;
    let mono = doc
        .add_builtin_font(BuiltinFont::Courier)
        .map_err(|e| EventLogError::Other(format!("pdf font: {e}")))?;

    let mut y = PAGE_H - MARGIN;

    // Header — brand title in CloudSaw red. printpdf 0.7's `Line`
    // surface dropped fill/stroke flags so we keep the header as
    // text-only (the existing scan reports do the same — no
    // colored shapes, just typography), which keeps the renderer
    // simple and forward-compatible.
    {
        let layer = doc.get_page(page_idx).get_layer(layer_idx);
        layer.set_fill_color(printpdf::Color::Rgb(printpdf::Rgb::new(
            213.0 / 255.0,
            40.0 / 255.0,
            54.0 / 255.0,
            None,
        )));
        layer.use_text("CloudSaw — Activity log", 18.0, Mm(MARGIN), Mm(y), &bold);
        y -= 8.0;
        layer.set_fill_color(printpdf::Color::Rgb(printpdf::Rgb::new(
            0.0, 0.0, 0.0, None,
        )));
    }

    // Meta.
    {
        let layer = doc.get_page(page_idx).get_layer(layer_idx);
        layer.use_text(
            format!(
                "Generated {}",
                Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
            ),
            META_FONT_SIZE,
            Mm(MARGIN),
            Mm(y),
            &regular,
        );
        y -= LINE_H;
        layer.use_text(
            format!(
                "{} entr{} included",
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" }
            ),
            META_FONT_SIZE,
            Mm(MARGIN),
            Mm(y),
            &regular,
        );
        y -= LINE_H * 2.0;
    }

    // Entries — one compact row per event.
    for e in entries {
        if y < MARGIN + 18.0 {
            // New page.
            let (new_page, new_layer) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "page-extra");
            page_idx = new_page;
            layer_idx = new_layer;
            y = PAGE_H - MARGIN;
        }
        let layer = doc.get_page(page_idx).get_layer(layer_idx);
        let when = e.occurred_at.format("%Y-%m-%d %H:%M:%SZ").to_string();
        let acct = e.aws_account_id_masked.as_deref().unwrap_or("—");
        let count = e
            .item_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string());
        let header_line = format!(
            "{} · {} · acct={} · count={}",
            when,
            e.kind.as_str(),
            acct,
            count
        );
        layer.use_text(header_line, BODY_FONT_SIZE, Mm(MARGIN), Mm(y), &bold);
        y -= LINE_H;
        let summary = truncate(&e.summary, 110);
        layer.use_text(summary, BODY_FONT_SIZE, Mm(MARGIN + 4.0), Mm(y), &regular);
        y -= LINE_H;
        if let Some(detail) = &e.detail {
            let trimmed = truncate(detail, 110);
            layer.use_text(trimmed, META_FONT_SIZE, Mm(MARGIN + 4.0), Mm(y), &mono);
            y -= LINE_H;
        }
        y -= 1.5;
    }

    // Footer — brand line + license on every page (just the last one
    // for simplicity).
    {
        let layer = doc.get_page(page_idx).get_layer(layer_idx);
        let footer_y = MARGIN - 6.0;
        layer.use_text(
            format!("CloudSaw  ·  License: {APP_LICENSE}"),
            META_FONT_SIZE,
            Mm(MARGIN),
            Mm(footer_y.max(4.0)),
            &regular,
        );
    }

    doc.save_to_bytes()
        .map_err(|e| EventLogError::Other(format!("pdf save: {e}")))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// --- Excel -----------------------------------------------------------------

fn render_xlsx(entries: &[EventLogEntry]) -> Result<Vec<u8>, EventLogError> {
    use rust_xlsxwriter::{Color, Format, FormatAlign, Workbook};

    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    sheet
        .set_name("Activity log")
        .map_err(|e| EventLogError::Other(format!("xlsx name: {e}")))?;

    // Brand banner row: span A1:E1 with the saw-red color + white text.
    let banner_fmt = Format::new()
        .set_background_color(Color::RGB(0x00D5_2836))
        .set_font_color(Color::White)
        .set_bold()
        .set_font_size(14.0)
        .set_align(FormatAlign::VerticalCenter)
        .set_align(FormatAlign::Left);
    sheet
        .merge_range(0, 0, 0, 4, "CloudSaw — Activity log", &banner_fmt)
        .map_err(|e| EventLogError::Other(format!("xlsx merge: {e}")))?;
    sheet
        .set_row_height(0, 28.0)
        .map_err(|e| EventLogError::Other(format!("xlsx row h: {e}")))?;

    // Meta line on row 2.
    let meta_fmt = Format::new()
        .set_italic()
        .set_font_color(Color::RGB(0x004C_525C));
    sheet
        .write_string_with_format(
            1,
            0,
            format!(
                "Generated {} · {} entr{} included",
                Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" },
            ),
            &meta_fmt,
        )
        .map_err(|e| EventLogError::Other(format!("xlsx write: {e}")))?;

    // Column headers on row 4 (leave row 3 blank for breathing room).
    let header_fmt = Format::new()
        .set_background_color(Color::RGB(0x00ED_EFF3))
        .set_font_color(Color::RGB(0x0036_3B43))
        .set_bold()
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let headers = ["When (UTC)", "Kind", "Summary", "Detail", "Account"];
    for (col, h) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(3, col as u16, *h, &header_fmt)
            .map_err(|e| EventLogError::Other(format!("xlsx hdr: {e}")))?;
    }

    let mono_fmt = Format::new().set_font_name("Consolas").set_font_size(10.0);
    let body_fmt = Format::new().set_font_size(10.0);
    let count_fmt = Format::new()
        .set_font_size(10.0)
        .set_align(FormatAlign::Right);

    for (i, e) in entries.iter().enumerate() {
        let row = 4 + i as u32;
        sheet
            .write_string_with_format(
                row,
                0,
                e.occurred_at.format("%Y-%m-%d %H:%M:%SZ").to_string(),
                &mono_fmt,
            )
            .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        sheet
            .write_string_with_format(row, 1, e.kind.as_str(), &mono_fmt)
            .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        sheet
            .write_string_with_format(row, 2, &e.summary, &body_fmt)
            .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        sheet
            .write_string_with_format(row, 3, e.detail.as_deref().unwrap_or(""), &body_fmt)
            .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        sheet
            .write_string_with_format(
                row,
                4,
                e.aws_account_id_masked.as_deref().unwrap_or(""),
                &mono_fmt,
            )
            .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        if let Some(n) = e.item_count {
            sheet
                .write_number_with_format(row, 5, n as f64, &count_fmt)
                .map_err(|err| EventLogError::Other(format!("xlsx body: {err}")))?;
        }
    }

    // Column widths — wide enough that the common cases don't wrap.
    sheet.set_column_width(0, 22.0).ok();
    sheet.set_column_width(1, 22.0).ok();
    sheet.set_column_width(2, 60.0).ok();
    sheet.set_column_width(3, 40.0).ok();
    sheet.set_column_width(4, 14.0).ok();
    sheet.set_column_width(5, 8.0).ok();

    // Footer band at the bottom of the data with the license string.
    let footer_row = 4 + entries.len() as u32 + 1;
    let footer_fmt = Format::new()
        .set_background_color(Color::RGB(0x00D5_2836))
        .set_font_color(Color::White)
        .set_bold()
        .set_align(FormatAlign::VerticalCenter)
        .set_align(FormatAlign::Left);
    sheet
        .merge_range(
            footer_row,
            0,
            footer_row,
            4,
            &format!("CloudSaw  ·  License: {APP_LICENSE}"),
            &footer_fmt,
        )
        .map_err(|e| EventLogError::Other(format!("xlsx footer: {e}")))?;
    sheet
        .set_row_height(footer_row, 22.0)
        .map_err(|e| EventLogError::Other(format!("xlsx footer h: {e}")))?;

    wb.save_to_buffer()
        .map_err(|e| EventLogError::Other(format!("xlsx save: {e}")))
}
