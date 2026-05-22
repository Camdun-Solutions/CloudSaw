// GitHub Issues REST API client. CLAUDE.md §4.3 + Contract 12
// §Constraints:
//
//   * The PAT is fetched on demand and held only for the duration of
//     the single API call. The `Zeroizing<String>` wrapper ensures the
//     buffer is wiped when the function returns.
//   * The PAT is sent ONLY in the `Authorization` header. It is NEVER
//     in the URL, NEVER logged.
//   * Every error from `reqwest` is collapsed into the typed
//     `GithubError` enum. No raw provider text crosses IPC.
//   * The client is `reqwest::blocking` to stay consistent with the
//     knowledgebase refresh path — both surfaces sit behind a sync IPC
//     command and run on a tokio blocking worker.

use std::time::Duration;

use serde::Serialize;
use zeroize::Zeroizing;

use super::error::GithubError;
use super::pat;
use super::types::{IssueCreated, RepoSelection};

const API_BASE: &str = "https://api.github.com";

/// Transport abstraction so tests can inject a fixture.
pub trait Transport: Send + Sync {
    fn post_issue(
        &self,
        repo: &RepoSelection,
        token: &str,
        payload: &IssuePayload<'_>,
    ) -> Result<IssueCreated, GithubError>;
}

#[derive(Debug, Serialize)]
pub struct IssuePayload<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub labels: &'a [String],
}

/// Production HTTPS transport.
pub struct ReqwestTransport;

impl Transport for ReqwestTransport {
    fn post_issue(
        &self,
        repo: &RepoSelection,
        token: &str,
        payload: &IssuePayload<'_>,
    ) -> Result<IssueCreated, GithubError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("CloudSaw/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| GithubError::Network)?;

        let url = format!(
            "{base}/repos/{owner}/{name}/issues",
            base = API_BASE,
            owner = repo.owner,
            name = repo.name,
        );

        let resp = client
            .post(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(token)
            .json(payload)
            .send()
            .map_err(|_| GithubError::Network)?;

        let status = resp.status();
        if status.is_success() {
            let body: GithubIssueResponse = resp
                .json()
                .map_err(|_| GithubError::Server(status.as_u16()))?;
            return Ok(IssueCreated {
                repo: repo.clone(),
                issue_number: body.number,
                issue_url: body.html_url,
            });
        }

        match status.as_u16() {
            401 | 403 => {
                // GitHub uses 403 for both rate-limit and permission
                // denial. Inspect headers to disambiguate.
                let is_rate_limit = resp
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| n == 0)
                    .unwrap_or(false);
                if is_rate_limit {
                    Err(GithubError::RateLimited)
                } else {
                    Err(GithubError::TokenInvalid)
                }
            }
            429 => Err(GithubError::RateLimited),
            other => Err(GithubError::Server(other)),
        }
    }
}

#[derive(serde::Deserialize)]
struct GithubIssueResponse {
    number: u32,
    html_url: String,
}

/// Public entry point used by the IPC bridge. Fetches the PAT,
/// dispatches through the production transport, then drops the PAT
/// before returning.
pub fn create_issue(
    repo: &RepoSelection,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<IssueCreated, GithubError> {
    let token = pat::get()?.ok_or(GithubError::NoToken)?;
    create_issue_with(&ReqwestTransport, repo, &token, title, body, labels)
}

/// Test seam — accepts an injected transport. Production callers use
/// `create_issue` above.
pub fn create_issue_with(
    transport: &dyn Transport,
    repo: &RepoSelection,
    token: &Zeroizing<String>,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<IssueCreated, GithubError> {
    let payload = IssuePayload {
        title,
        body,
        labels,
    };
    transport.post_issue(repo, token, &payload)
}

/// Build the prefilled new-issue URL the browser fallback opens. GitHub
/// reads `title`, `body`, and `labels` from the query string. Per
/// Contract 12 §Constraints the PAT NEVER appears in any URL.
pub fn browser_fallback_url(
    repo: &RepoSelection,
    title: &str,
    body: &str,
    labels: &[String],
) -> String {
    let labels_joined = labels.join(",");
    let mut url = format!(
        "https://github.com/{}/{}/issues/new",
        repo.owner, repo.name,
    );
    url.push_str(&format!(
        "?title={}&body={}",
        url_encode(title),
        url_encode(body),
    ));
    if !labels_joined.is_empty() {
        url.push_str(&format!("&labels={}", url_encode(&labels_joined)));
    }
    url
}

/// Tiny pure-ASCII URL encoder — we don't need a dep for this. Encodes
/// every byte that isn't in the unreserved set defined by RFC 3986.
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for b in input.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_preserves_unreserved_and_escapes_rest() {
        assert_eq!(url_encode("hello-world"), "hello-world");
        assert_eq!(url_encode("foo bar"), "foo%20bar");
        assert_eq!(url_encode("a/b"), "a%2Fb");
    }

    #[test]
    fn browser_fallback_url_includes_title_body_labels() {
        let url = browser_fallback_url(
            &RepoSelection {
                owner: "acme".into(),
                name: "repo".into(),
            },
            "Bug: scan froze",
            "Body text",
            &["bug".to_string(), "needs-triage".to_string()],
        );
        assert!(url.starts_with("https://github.com/acme/repo/issues/new"));
        assert!(url.contains("title=Bug%3A%20scan%20froze"));
        assert!(url.contains("body=Body%20text"));
        assert!(url.contains("labels=bug%2Cneeds-triage"));
    }

    #[test]
    fn browser_fallback_url_never_contains_token_or_authorization() {
        let url = browser_fallback_url(
            &RepoSelection {
                owner: "acme".into(),
                name: "repo".into(),
            },
            "t",
            "b",
            &[],
        );
        // Defense in depth: the URL builder takes no token argument,
        // so it can't smuggle one. Belt-and-suspenders assertion.
        assert!(!url.to_lowercase().contains("authorization"));
        assert!(!url.contains("ghp_"));
        assert!(!url.contains("github_pat_"));
    }
}
