// Public data types crossing the IPC boundary.
//
// Per CLAUDE.md §4.1, IPC payloads are plain serializable structs — no AWS
// SDK types, no credential-bearing types. Every field below is a primitive
// or a deliberately enumerated tag (Environment, ScanOutcome).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One row from the `accounts` table, ready to render in the UI.
///
/// `aws_account_id` is the full 12-digit value. UI masking (last 4) is a
/// frontend concern toggled by the `reveal_full_ids` setting; logs mask
/// regardless (CLAUDE.md §4.4). The row reaches IPC unredacted so the user,
/// who already owns this data, can see exactly which account is which.
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub aws_account_id: String,
    pub label: String,
    pub profile_name: String,
    pub environment: Environment,
    pub role_provisioned: bool,
    pub role_provisioned_at: Option<DateTime<Utc>>,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub last_scan_status: Option<ScanOutcome>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Dev,
    Staging,
    Prod,
    Other,
}

impl Environment {
    pub fn as_str(self) -> &'static str {
        match self {
            Environment::Dev => "dev",
            Environment::Staging => "staging",
            Environment::Prod => "prod",
            Environment::Other => "other",
        }
    }

    /// Map a stored string back to the enum. Unknown values fall back to
    /// `Other` so a forward-compatible row written by a newer build doesn't
    /// crash the parser — we surface "other" until that build supports it.
    pub fn from_storage(s: &str) -> Environment {
        match s {
            "dev" => Environment::Dev,
            "staging" => Environment::Staging,
            "prod" => Environment::Prod,
            _ => Environment::Other,
        }
    }
}

/// Last scan outcome, kept tiny on purpose. Real scan-result types land in
/// Contract 06; this is just the status badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanOutcome {
    Success,
    Failure,
    PartialSuccess,
    Unknown,
}

impl ScanOutcome {
    pub fn from_storage(s: String) -> ScanOutcome {
        match s.as_str() {
            "success" => ScanOutcome::Success,
            "failure" => ScanOutcome::Failure,
            "partial_success" => ScanOutcome::PartialSuccess,
            _ => ScanOutcome::Unknown,
        }
    }
}

/// Internal row used by `storage::insert`. The aws_account_id has been
/// verified against STS before this struct is constructed.
#[derive(Debug, Clone)]
pub struct AccountRecord {
    pub aws_account_id: String,
    pub label: String,
    pub profile_name: String,
    pub environment: Environment,
}

/// IPC input: payload of `accounts_add_account`. The frontend supplies the
/// label, profile, and environment tag. The backend computes the AWS account
/// ID via STS — the frontend cannot smuggle in a fake one.
#[derive(Debug, Clone, Deserialize)]
pub struct AddAccountInput {
    pub label: String,
    pub profile_name: String,
    pub environment: Environment,
}

/// IPC input: payload of `accounts_update_account`. `aws_account_id`
/// identifies the row; the other fields are the new values. Changing
/// `profile_name` re-runs STS verification — if the resolved account
/// ID differs from the existing row, `AccountsError::AwsAccountIdMismatch`
/// is raised and nothing is written.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAccountInput {
    pub aws_account_id: String,
    pub label: String,
    pub profile_name: String,
    pub environment: Environment,
}

/// Account-removal impact preview. The UI shows this before the user
/// confirms — Contract 04 §Constraints requires that removal "clearly state
/// what associated local data (scans, findings, tf-work) will be affected".
///
/// Right now those tables don't exist (Contracts 06/07/11 add them) but the
/// shape is in place so later contracts can drop in real counts without an
/// IPC break.
#[derive(Debug, Clone, Serialize)]
pub struct RemovalImpact {
    pub scans: u64,
    pub findings: u64,
    pub tf_work: u64,
    pub was_active: bool,
}

/// Display-preference payload. One boolean today, extensible later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsDisplaySettings {
    pub reveal_full_ids: bool,
}
