// Markdown article parser.
//
// Bundled and remote articles share the same shape: an optional H1 title
// followed by H2-delimited sections. Each H2's text becomes a section key;
// known section names (Description, Risk, …) map onto typed fields on the
// returned `KnowledgeArticle`. Unknown H2 headings survive in
// `unmatched_sections` for forward compatibility (Contract 08 §Edge Cases).
//
// The parser is pure: identical input always produces identical output. It
// reads no time, no env, and no disk. Markdown is returned as raw strings
// per Contract 08 §Constraints — rendering happens in the frontend, which
// is the layer responsible for sanitization (Contract 09).

use std::collections::BTreeMap;

use super::types::{KnowledgeArticle, KnowledgeSource};

const H1_PREFIX: &str = "# ";
const H2_PREFIX: &str = "## ";

/// Parse a markdown article body for `finding_id`. The article is always
/// considered "matched" — callers detect the missing-article case BEFORE
/// calling this (by checking the bundled/remote index) and use
/// `KnowledgeArticle::default_for` instead.
pub fn parse_article(finding_id: &str, source: KnowledgeSource, body: &str) -> KnowledgeArticle {
    let mut title: Option<String> = None;
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut current_heading: Option<String> = None;
    let mut current_body: Vec<&str> = Vec::new();

    for line in body.lines() {
        if let Some(rest) = line.strip_prefix(H2_PREFIX) {
            // Flush the prior section before opening the new one.
            if let Some(heading) = current_heading.take() {
                sections.insert(heading, join_trimmed(&current_body));
                current_body.clear();
            }
            current_heading = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix(H1_PREFIX) {
            // Title is only recognized before the first H2 so a body
            // can't accidentally overwrite it.
            if current_heading.is_none() && title.is_none() {
                title = Some(rest.trim().to_string());
                continue;
            }
        }
        if current_heading.is_some() {
            current_body.push(line);
        }
    }
    if let Some(heading) = current_heading.take() {
        sections.insert(heading, join_trimmed(&current_body));
    }

    // Pluck known sections out; the remainder becomes unmatched.
    let description = take_section(&mut sections, "Description");
    let risk = take_section(&mut sections, "Risk");
    let detection_logic = take_section(&mut sections, "Detection Logic");
    let remediation = take_section(&mut sections, "Remediation");
    let terraform_fix = take_section(&mut sections, "Terraform Fix");
    let aws_cli_fix = take_section(&mut sections, "AWS CLI Fix");
    let false_positives = take_section(&mut sections, "False Positives");

    KnowledgeArticle {
        finding_id: finding_id.to_string(),
        matched: true,
        source,
        title: title.unwrap_or_else(|| finding_id.to_string()),
        description,
        risk,
        detection_logic,
        remediation,
        terraform_fix,
        aws_cli_fix,
        false_positives,
        unmatched_sections: sections,
    }
}

/// Extract just the first H1 / title or fall back to the finding id. Used
/// by `list_articles` so we don't materialize the entire article body for
/// each entry.
pub fn extract_title(finding_id: &str, body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(H1_PREFIX) {
            return rest.trim().to_string();
        }
        if trimmed.starts_with(H2_PREFIX) {
            // First H2 reached without finding an H1 — give up rather than
            // walking the rest of the file.
            break;
        }
    }
    finding_id.to_string()
}

fn take_section(sections: &mut BTreeMap<String, String>, name: &str) -> String {
    sections.remove(name).unwrap_or_default()
}

fn join_trimmed(lines: &[&str]) -> String {
    let mut joined = lines.join("\n");
    // Trim leading blank lines, keep internal whitespace untouched.
    while joined.starts_with('\n') {
        joined.remove(0);
    }
    while joined.ends_with('\n') {
        joined.pop();
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_expected_sections() {
        let body = "# IAM password policy lacks minimum length\n\n\
            ## Description\nA description paragraph.\n\n\
            ## Risk\nThe risk.\n\n\
            ## Detection Logic\nHow we detect.\n\n\
            ## Remediation\nFix it.\n\n\
            ## Terraform Fix\n```hcl\nresource\n```\n\n\
            ## AWS CLI Fix\n```sh\naws ...\n```\n\n\
            ## False Positives\nNone usually.";
        let a = parse_article("iam-test", KnowledgeSource::Bundled, body);
        assert!(a.matched);
        assert_eq!(a.title, "IAM password policy lacks minimum length");
        assert_eq!(a.description, "A description paragraph.");
        assert_eq!(a.risk, "The risk.");
        assert_eq!(a.detection_logic, "How we detect.");
        assert_eq!(a.remediation, "Fix it.");
        assert!(a.terraform_fix.contains("resource"));
        assert!(a.aws_cli_fix.contains("aws ..."));
        assert_eq!(a.false_positives, "None usually.");
        assert!(a.unmatched_sections.is_empty());
    }

    #[test]
    fn missing_h2_sections_load_as_empty_strings() {
        let body = "# Title only\n\n## Description\nOnly description.";
        let a = parse_article("x", KnowledgeSource::Bundled, body);
        assert_eq!(a.description, "Only description.");
        assert_eq!(a.risk, "");
        assert_eq!(a.remediation, "");
        assert_eq!(a.terraform_fix, "");
    }

    #[test]
    fn unexpected_h2_is_captured_in_unmatched_sections() {
        let body = "## Description\nd\n\n## Compliance Mapping\nSOC 2 CC6.1.\n\n## Remediation\nr";
        let a = parse_article("x", KnowledgeSource::Bundled, body);
        assert_eq!(
            a.unmatched_sections
                .get("Compliance Mapping")
                .map(String::as_str),
            Some("SOC 2 CC6.1.")
        );
        assert_eq!(a.description, "d");
        assert_eq!(a.remediation, "r");
    }

    #[test]
    fn extract_title_returns_finding_id_when_no_h1() {
        assert_eq!(
            extract_title("iam-foo", "## Description\nd"),
            "iam-foo".to_string()
        );
        assert_eq!(
            extract_title("iam-foo", "# Big title here\n\n## Description"),
            "Big title here".to_string()
        );
    }
}
