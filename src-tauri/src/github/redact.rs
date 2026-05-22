// Redaction rules applied to every string that lands in a diagnostic
// bundle, an issue body, or any other GitHub-bound payload. The rules
// are intentionally aggressive and defensive: we'd rather over-redact
// than risk leaking. CLAUDE.md §4.4 + Contract 12 §Constraints:
//
//   * Every 12-digit AWS account ID is masked to the last 4 digits.
//   * Every AWS ARN is truncated at the resource path.
//   * Any `AKIA…` / `ASIA…` access-key prefix is removed entirely.
//   * Any sequence that looks like a GitHub PAT (`ghp_`, `github_pat_`,
//     `ghs_`, `gho_`, `ghu_`, `ghr_`) is removed.
//   * Common bearer-token / authorization headers are blanked.
//   * Lines containing `password`, `passwd`, `secret`, `api_key`,
//     `bearer ` are dropped entirely.
//
// These rules apply to log lines AND to the user-supplied "what were
// you doing?" notes the user pastes into the error dialog — the user
// might paste a stack trace verbatim.

use std::sync::OnceLock;

use regex::Regex;

fn account_id_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b\d{12}\b").unwrap())
}

fn arn_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // `arn:partition:service:region:account-id:resource[/qualifier]`
        // Match through the resource-type token and truncate the rest.
        // The tail set excludes whitespace, quote characters, comma, and
        // semicolon so we stop at the end of the ARN token.
        Regex::new(
            r#"arn:(?P<partition>[a-z0-9\-]+):(?P<service>[a-zA-Z0-9\-]+):(?P<region>[a-zA-Z0-9\-]*):(?P<account>\d{12}):(?P<rest>[^\s'",;]+)"#,
        )
        .unwrap()
    })
}

fn access_key_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(?:AKIA|ASIA|AGPA|AROA|AIDA)[A-Z0-9]{12,}\b").unwrap())
}

fn github_pat_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(?:ghp|ghs|gho|ghu|ghr)_[A-Za-z0-9_]{20,}\b").unwrap())
}

fn github_pat_finegrained_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b").unwrap())
}

fn bearer_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)bearer\s+[A-Za-z0-9._\-]+").unwrap())
}

/// Returns `true` when the line MUST be dropped entirely rather than
/// individually masked — any line whose surface mentions a credential
/// keyword. We treat these aggressively because a line like
/// `secret: hunter2` would survive token-level redaction otherwise.
pub fn line_looks_credential_bearing(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    const TRIGGERS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "api_key",
        "api-key",
        "apikey",
        "session_token",
        "session-token",
        "x-amz-security-token",
        "authorization:",
    ];
    TRIGGERS.iter().any(|t| lower.contains(t))
}

/// Apply every token-level rule to a single input. Lines that should
/// be dropped wholesale are caught by `line_looks_credential_bearing`
/// before they reach this function.
pub fn redact_line(input: &str) -> String {
    let mut s = input.to_string();

    // Order matters: PATs first (they're high-entropy and could be
    // mistaken for arbitrary identifiers), then access keys, then
    // bearer/header forms, then ARNs (which contain account IDs),
    // then bare account IDs.
    s = github_pat_finegrained_re()
        .replace_all(&s, "[REDACTED-PAT]")
        .to_string();
    s = github_pat_re()
        .replace_all(&s, "[REDACTED-PAT]")
        .to_string();
    s = access_key_re()
        .replace_all(&s, "[REDACTED-KEY]")
        .to_string();
    s = bearer_re().replace_all(&s, "Bearer [REDACTED]").to_string();
    s = arn_re()
        .replace_all(&s, |c: &regex::Captures<'_>| {
            let partition = c.name("partition").map(|m| m.as_str()).unwrap_or("aws");
            let service = c.name("service").map(|m| m.as_str()).unwrap_or("?");
            let region = c.name("region").map(|m| m.as_str()).unwrap_or("");
            let account = c
                .name("account")
                .map(|m| m.as_str())
                .unwrap_or("000000000000");
            let masked = format!(
                "arn:{partition}:{service}:{region}:****{tail}:[truncated]",
                tail = &account[account.len().saturating_sub(4)..],
            );
            masked
        })
        .to_string();
    s = account_id_re()
        .replace_all(&s, |c: &regex::Captures<'_>| {
            let m = c.get(0).unwrap().as_str();
            format!("****{}", &m[m.len() - 4..])
        })
        .to_string();
    s
}

/// Convenience for redacting a multi-line block: drops credential-bearing
/// lines outright, then applies token rules to the rest. Empty lines and
/// runs of blanks are preserved so a real-world stack trace stays
/// readable.
pub fn redact_block(input: &str) -> String {
    input
        .lines()
        .filter(|l| !line_looks_credential_bearing(l))
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_bare_aws_account_id() {
        assert_eq!(
            redact_line("logged in as 111122223333"),
            "logged in as ****3333"
        );
    }

    #[test]
    fn truncates_arn_keeping_partition_service_region_and_account_tail() {
        let r = redact_line("role arn:aws:iam::111122223333:role/CloudSawScanner failed");
        assert!(r.contains("arn:aws:iam::****3333:[truncated]"));
        assert!(!r.contains("CloudSawScanner"));
        assert!(!r.contains("111122223333"));
    }

    #[test]
    fn redacts_access_key_id() {
        let r = redact_line("got AKIAEXAMPLEKEY1234 from STS");
        assert_eq!(r, "got [REDACTED-KEY] from STS");
    }

    #[test]
    fn redacts_github_pat_classic_and_finegrained() {
        let classic = redact_line("token=ghp_abcdef0123456789abcdef0123");
        assert!(classic.contains("[REDACTED-PAT]"));
        let fine = redact_line("token=github_pat_11AAA1111A0123456789aaaaaa0123456789");
        assert!(fine.contains("[REDACTED-PAT]"));
    }

    #[test]
    fn redacts_bearer_header() {
        let r = redact_line("Authorization: Bearer eyJhbGc.foo.bar");
        // The whole line is dropped by line_looks_credential_bearing in
        // block mode, but token mode masks the bearer in place.
        assert!(r.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn line_drop_filter_catches_secret_keywords() {
        assert!(line_looks_credential_bearing("password: hunter2"));
        assert!(line_looks_credential_bearing("API_KEY=foo"));
        assert!(line_looks_credential_bearing("AUTHORIZATION: Bearer xyz"));
        assert!(!line_looks_credential_bearing("scan complete"));
    }

    #[test]
    fn block_drops_secret_lines_and_masks_the_rest() {
        let input = "\
            scan started in 111122223333
password: hunter2
arn:aws:iam::111122223333:role/X
done";
        let out = redact_block(input);
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("111122223333"));
        assert!(out.contains("****3333"));
        assert!(out.contains("done"));
    }
}
