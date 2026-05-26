// SQLite persistence for the scanner_role module.
//
// Phase 2 moves these functions verbatim from the deleted
// `terraform::storage`. The schema is unchanged — migration 0004 still
// owns the four `accounts` columns the connect flow writes:
//
//   * `external_id`             — confused-deputy guard for the
//                                  trust policy. Persisted because the
//                                  user MUST use the exact value in
//                                  their role's trust-policy condition.
//   * `policy_variant`          — which AWS-managed policy is attached.
//   * `last_provisioning_error` — stable error code from the most
//                                  recent failed connect attempt.
//   * `scanner_role_arn`        — captured from the user's connect
//                                  submission.
//
// CLAUDE.md §4.5: every SQL here uses parameterized queries. No string
// interpolation, anywhere.

use chrono::Utc;
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::ScannerRoleError;
use super::types::PolicyVariant;
use crate::db::paths::app_data_dir;

fn db_path() -> Result<std::path::PathBuf, ScannerRoleError> {
    Ok(app_data_dir()
        .map_err(|e| ScannerRoleError::Db(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, ScannerRoleError> {
    Connection::open(db_path()?).map_err(ScannerRoleError::from)
}

/// Read the per-account external_id, generating one if this is the
/// first connect attempt for the account. 32 hex chars (128 bits) —
/// well above AWS's 16-char minimum and short enough for the recipe
/// blocks (Console / Terraform / CFN / CLI) to stay readable.
///
/// External IDs are configuration, not credentials (they only guard
/// against the confused-deputy attack pattern); persisting in SQLite
/// is acceptable. See CLAUDE.md §4.3 — credentials never land in
/// SQLite, but the external ID is bound to the trust policy and
/// cannot, alone, authenticate anyone.
pub fn ensure_external_id(aws_account_id: &str) -> Result<String, ScannerRoleError> {
    let conn = open()?;
    let existing: Option<Option<String>> = conn
        .query_row(
            "SELECT external_id FROM accounts WHERE aws_account_id = ?1",
            params![aws_account_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    let row = existing.ok_or(ScannerRoleError::InvalidInput("aws_account_id"))?;
    if let Some(id) = row {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let id = generate_external_id();
    conn.execute(
        "UPDATE accounts SET external_id = ?1, updated_at = ?2 WHERE aws_account_id = ?3",
        params![id, Utc::now().to_rfc3339(), aws_account_id],
    )?;
    Ok(id)
}

fn generate_external_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Persist a successful connect. Writes the role ARN + policy variant,
/// marks `role_provisioned = 1`, clears any prior error.
pub fn record_provisioned(
    aws_account_id: &str,
    role_arn: &str,
    policy_variant: PolicyVariant,
) -> Result<(), ScannerRoleError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE accounts
            SET role_provisioned = 1,
                role_provisioned_at = ?1,
                scanner_role_arn = ?2,
                policy_variant = ?3,
                last_provisioning_error = NULL,
                updated_at = ?4
          WHERE aws_account_id = ?5",
        params![
            now,
            role_arn,
            policy_variant.as_storage_str(),
            now,
            aws_account_id
        ],
    )?;
    Ok(())
}

/// Persist a failed connect attempt. Records the stable error code and
/// the attempt time. `role_provisioned` is left unchanged — a failed
/// re-connect against an already-connected role does not invalidate
/// the prior success.
pub fn record_failure(aws_account_id: &str, error_code: &str) -> Result<(), ScannerRoleError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE accounts
            SET last_provisioning_error = ?1, updated_at = ?2
          WHERE aws_account_id = ?3",
        params![error_code, now, aws_account_id],
    )?;
    Ok(())
}

/// Read enough of the row to drive `scanner_role_status()`.
pub struct ProvisioningRow {
    pub role_provisioned: bool,
    pub role_provisioned_at: Option<String>,
    pub scanner_role_arn: Option<String>,
    pub policy_variant: Option<String>,
    pub last_provisioning_error: Option<String>,
    pub updated_at: String,
}

pub fn provisioning_row(aws_account_id: &str) -> Result<ProvisioningRow, ScannerRoleError> {
    let conn = open()?;
    conn.query_row(
        "SELECT role_provisioned, role_provisioned_at, scanner_role_arn,
                policy_variant, last_provisioning_error, updated_at
           FROM accounts
          WHERE aws_account_id = ?1",
        params![aws_account_id],
        |row| {
            Ok(ProvisioningRow {
                role_provisioned: row.get::<_, i64>(0)? != 0,
                role_provisioned_at: row.get(1)?,
                scanner_role_arn: row.get(2)?,
                policy_variant: row.get(3)?,
                last_provisioning_error: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or(ScannerRoleError::InvalidInput("aws_account_id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_external_ids_are_hex_and_long_enough() {
        let id = generate_external_id();
        assert_eq!(id.len(), 32); // 16 bytes -> 32 hex chars
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        let other = generate_external_id();
        assert_ne!(id, other);
    }
}
