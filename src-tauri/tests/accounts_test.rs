// Integration tests for the accounts module (Contract 04 + 04-QA).
//
// Each test runs the real migration runner against a temp SQLite database
// pointed to by `CLOUDSAW_DATA_DIR_OVERRIDE`, then exercises the public
// `accounts::*` surface end-to-end. `add_account`/`update_account` call STS
// internally, which would normally need a live AWS account; here we exercise
// them via the storage layer directly (`storage::insert`/`storage::delete`)
// for happy paths, and via the public API for the failure paths we can
// trigger without network (validation, missing profile, …).
//
// Tests serialize through a module-level mutex because they share the
// `CLOUDSAW_DATA_DIR_OVERRIDE` and `AWS_CONFIG_FILE` env vars.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use cloudsaw_lib::accounts::error::AccountsError;
use cloudsaw_lib::accounts::storage;
use cloudsaw_lib::accounts::{
    self, types::AccountRecord, AccountsDisplaySettings, AddAccountInput, Environment,
    UpdateAccountInput,
};
use cloudsaw_lib::auth::AuthError;
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::errors::AppError;

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    aws_config_dir: PathBuf,
    prev_aws_config: Option<String>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-accounts-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();

        // Point AWS_CONFIG_FILE at a sandbox so auth::list_profiles can be
        // tested deterministically per test.
        let aws_dir = dir.join("aws");
        fs::create_dir_all(&aws_dir).unwrap();
        let prev_aws_config = std::env::var("AWS_CONFIG_FILE").ok();
        std::env::set_var("AWS_CONFIG_FILE", aws_dir.join("config"));

        Self {
            _guard: guard,
            dir,
            aws_config_dir: aws_dir,
            prev_aws_config,
        }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("db").join("cloudsaw.db")
    }

    fn write_aws_config(&self, body: &str) {
        fs::write(self.aws_config_dir.join("config"), body).unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        match &self.prev_aws_config {
            Some(v) => std::env::set_var("AWS_CONFIG_FILE", v),
            None => std::env::remove_var("AWS_CONFIG_FILE"),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn seed(label: &str, profile: &str, env: Environment, aws_id: &str) -> AccountRecord {
    AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: profile.to_string(),
        environment: env,
    }
}

// --- Happy Path ----------------------------------------------------------

#[test]
fn list_accounts_is_empty_on_a_fresh_install() {
    // QA state transition: "Zero accounts → one account added". Verifies the
    // zero-accounts initial condition the empty-state UI relies on.
    let _sb = Sandbox::new("zero");
    let list = accounts::list_accounts().unwrap();
    assert!(list.is_empty());
    assert!(accounts::get_active_account().unwrap().is_none());
}

#[test]
fn insert_and_list_round_trip() {
    let _sb = Sandbox::new("list");

    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();

    let list = accounts::list_accounts().unwrap();
    assert_eq!(list.len(), 2);
    // ORDER BY label ASC.
    assert_eq!(list[0].label, "dev");
    assert_eq!(list[1].label, "prod");
    assert_eq!(list[0].aws_account_id, "111122223333");
    assert_eq!(list[1].aws_account_id, "444455556666");
    assert_eq!(list[0].environment, Environment::Dev);
    assert_eq!(list[1].environment, Environment::Prod);
}

#[test]
fn get_returns_full_row() {
    let _sb = Sandbox::new("get");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    let row = accounts::get_account("111122223333").unwrap();
    assert_eq!(row.label, "dev");
    assert_eq!(row.profile_name, "dev-profile");
    assert_eq!(row.environment, Environment::Dev);
    assert!(!row.role_provisioned);
    assert!(row.last_scan_at.is_none());
}

#[test]
fn active_account_set_and_get() {
    let _sb = Sandbox::new("active");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();

    assert!(accounts::get_active_account().unwrap().is_none());
    accounts::set_active_account(Some("111122223333")).unwrap();
    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("111122223333")
    );

    // Clearing it.
    accounts::set_active_account(None).unwrap();
    assert!(accounts::get_active_account().unwrap().is_none());
}

#[test]
fn removing_active_account_clears_selection() {
    let _sb = Sandbox::new("rm-active");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();
    accounts::set_active_account(Some("111122223333")).unwrap();

    let impact = accounts::remove_account("111122223333").unwrap();
    assert!(
        impact.was_active,
        "removal of active account must report was_active=true"
    );

    // Active selection is cleared in the same transaction.
    assert!(accounts::get_active_account().unwrap().is_none());

    // Removed row is gone; other row survives.
    assert!(matches!(
        accounts::get_account("111122223333").unwrap_err(),
        AccountsError::NotFound
    ));
    assert!(accounts::get_account("444455556666").is_ok());
}

#[test]
fn removing_non_active_account_does_not_touch_active() {
    let _sb = Sandbox::new("rm-noop");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();
    accounts::set_active_account(Some("111122223333")).unwrap();

    let impact = accounts::remove_account("444455556666").unwrap();
    assert!(!impact.was_active);
    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("111122223333"),
        "active selection must be untouched when a non-active row is removed"
    );
}

// --- Error states --------------------------------------------------------

#[test]
fn duplicate_aws_account_id_is_rejected() {
    let _sb = Sandbox::new("dup-id");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    let err = storage::insert(&seed(
        "dev-alt",
        "dev-profile-2",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap_err();
    assert!(matches!(err, AccountsError::DuplicateAwsAccountId));
}

#[test]
fn duplicate_label_is_rejected() {
    let _sb = Sandbox::new("dup-label");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    let err = storage::insert(&seed(
        "dev",
        "other-profile",
        Environment::Dev,
        "444455556666",
    ))
    .unwrap_err();
    assert!(matches!(err, AccountsError::DuplicateLabel));
}

#[test]
fn get_unknown_account_returns_not_found() {
    let _sb = Sandbox::new("get-missing");
    let err = accounts::get_account("999988887777").unwrap_err();
    assert!(matches!(err, AccountsError::NotFound));
}

#[test]
fn remove_unknown_account_returns_not_found() {
    let _sb = Sandbox::new("rm-missing");
    let err = accounts::remove_account("999988887777").unwrap_err();
    assert!(matches!(err, AccountsError::NotFound));
}

#[test]
fn set_active_rejects_malformed_aws_account_id() {
    // The active-account ID never bypasses validation — even though the only
    // way a string lands here in normal flow is reading an existing row, we
    // defend against a malicious IPC client passing a non-12-digit value.
    let _sb = Sandbox::new("active-malformed");
    for bad in ["", "1234", "abcdefghijkl", "11112222333a", "1111222233334"] {
        let err = accounts::set_active_account(Some(bad)).unwrap_err();
        assert!(
            matches!(err, AccountsError::Internal(_)),
            "expected Internal(malformed_aws_account_id) for {bad:?}, got {err:?}"
        );
    }
}

#[test]
fn set_active_to_unknown_id_is_rejected() {
    let _sb = Sandbox::new("active-missing");
    let err = accounts::set_active_account(Some("999988887777")).unwrap_err();
    assert!(matches!(err, AccountsError::NotFound));
    // And nothing was written.
    assert!(accounts::get_active_account().unwrap().is_none());
}

#[test]
fn add_account_with_unknown_profile_does_not_persist_row() {
    let sb = Sandbox::new("add-missing-profile");
    // Empty AWS config — `auth::get_caller_identity` should surface
    // ProfileNotFound (or a network-class failure) and `add_account` must
    // refuse to write the row.
    sb.write_aws_config("[default]\nregion = us-east-1\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(accounts::add_account(AddAccountInput {
        label: "wont-save".to_string(),
        profile_name: "does-not-exist".to_string(),
        environment: Environment::Dev,
    }));
    let err = result.unwrap_err();
    assert!(
        matches!(err, AccountsError::Verification(AuthError::ProfileNotFound)),
        "missing profile must surface as Verification(ProfileNotFound), got {err:?}"
    );

    // No row was written for the attempt.
    let list = accounts::list_accounts().unwrap();
    assert!(
        list.is_empty(),
        "no row should be written when verification fails"
    );
}

#[test]
fn add_account_rejects_invalid_inputs_without_calling_sts() {
    let sb = Sandbox::new("invalid-inputs");
    sb.write_aws_config("[profile dev]\nregion = us-east-1\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Label rejected (empty).
    let err = rt
        .block_on(accounts::add_account(AddAccountInput {
            label: "".to_string(),
            profile_name: "dev".to_string(),
            environment: Environment::Dev,
        }))
        .unwrap_err();
    assert!(matches!(err, AccountsError::InvalidInput("label")));

    // Profile name with shell metacharacter rejected.
    let err = rt
        .block_on(accounts::add_account(AddAccountInput {
            label: "shellish".to_string(),
            profile_name: "$(whoami)".to_string(),
            environment: Environment::Dev,
        }))
        .unwrap_err();
    assert!(matches!(err, AccountsError::InvalidInput("profile_name")));
}

#[test]
fn update_rejects_duplicate_label_against_another_row() {
    let _sb = Sandbox::new("update-dup-label");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();

    let err = storage::update_fields("444455556666", "dev", "prod-profile", Environment::Prod)
        .unwrap_err();
    assert!(matches!(err, AccountsError::DuplicateLabel));
}

#[test]
fn update_with_same_label_on_same_row_is_allowed() {
    let _sb = Sandbox::new("update-same-label");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    let updated =
        storage::update_fields("111122223333", "dev", "dev-profile", Environment::Staging).unwrap();
    assert_eq!(updated.environment, Environment::Staging);
}

// --- Display settings ----------------------------------------------------

#[test]
fn reveal_full_ids_defaults_to_false_and_round_trips() {
    let _sb = Sandbox::new("reveal");
    let settings = accounts::get_display_settings().unwrap();
    assert!(
        !settings.reveal_full_ids,
        "default must be masked per Contract 04 §Constraints"
    );

    accounts::set_display_settings(AccountsDisplaySettings {
        reveal_full_ids: true,
    })
    .unwrap();
    assert!(accounts::get_display_settings().unwrap().reveal_full_ids);

    accounts::set_display_settings(AccountsDisplaySettings {
        reveal_full_ids: false,
    })
    .unwrap();
    assert!(!accounts::get_display_settings().unwrap().reveal_full_ids);
}

#[test]
fn mask_helper_only_exposes_last_four() {
    // Defense-in-depth check that the helper used for log redaction and
    // default UI display never returns more than the last 4 digits.
    let masked = accounts::mask_for_logs("111122223333");
    assert_eq!(masked, "****3333");
    assert!(!masked.contains("1111"));
    assert!(!masked.contains("2222"));
}

// --- Security checks (Contract 04 §Security Check) -----------------------

#[test]
fn first_insert_auto_promotes_to_active_via_helper() {
    // QA state transition: "Zero accounts → one account added → active
    // account set." `add_account` invokes `promote_if_no_active` after
    // `storage::insert`. Since add_account requires STS, we exercise the
    // helper directly (storage::insert + promote_if_no_active mirrors what
    // add_account does on a successful verification).
    let _sb = Sandbox::new("auto-promote-first");
    assert!(accounts::get_active_account().unwrap().is_none());

    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    accounts::promote_if_no_active("111122223333").unwrap();
    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("111122223333"),
        "the only account must become active when promote_if_no_active runs"
    );
}

#[test]
fn auto_promote_does_not_disturb_existing_active() {
    // Second add — must NOT change the active selection.
    let _sb = Sandbox::new("auto-promote-noop");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    accounts::set_active_account(Some("111122223333")).unwrap();

    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();
    accounts::promote_if_no_active("444455556666").unwrap();

    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("111122223333"),
        "active selection must persist across subsequent adds"
    );
}

#[test]
fn switching_active_account_changes_get_active_immediately() {
    // QA state transition: "Multiple accounts → active account switched →
    // scoped operations retarget". Since later contracts route scoped
    // operations through `get_active_account`, we verify the switch is
    // observable on the next read with no caching surprise.
    let _sb = Sandbox::new("switch");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();
    storage::insert(&seed(
        "prod",
        "prod-profile",
        Environment::Prod,
        "444455556666",
    ))
    .unwrap();

    accounts::set_active_account(Some("111122223333")).unwrap();
    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("111122223333")
    );

    accounts::set_active_account(Some("444455556666")).unwrap();
    assert_eq!(
        accounts::get_active_account().unwrap().as_deref(),
        Some("444455556666"),
        "next read after switch must reflect the new active account"
    );
}

#[test]
fn accounts_table_contains_no_credential_columns() {
    let sb = Sandbox::new("schema");
    let conn = Connection::open(sb.db_path()).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('accounts')")
        .unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let forbidden = [
        "access_key",
        "aws_access_key_id",
        "secret_key",
        "aws_secret_access_key",
        "session_token",
        "aws_session_token",
        "password",
        "secret",
        "token",
    ];
    for col in &cols {
        let lower = col.to_ascii_lowercase();
        for bad in &forbidden {
            assert!(
                !lower.contains(bad),
                "accounts schema must not contain credential column {col}"
            );
        }
    }
}

#[test]
fn account_id_mismatch_maps_to_stable_code() {
    let mapped: AppError = AccountsError::AwsAccountIdMismatch.into();
    assert_eq!(mapped.code(), "aws_account_id_mismatch");

    let mapped: AppError = AccountsError::DuplicateAwsAccountId.into();
    assert_eq!(mapped.code(), "duplicate_aws_account_id");

    let mapped: AppError = AccountsError::DuplicateLabel.into();
    assert_eq!(mapped.code(), "duplicate_label");

    let mapped: AppError = AccountsError::NotFound.into();
    assert_eq!(mapped.code(), "account_not_found");
}

// --- Update path (synchronous when profile is unchanged) -----------------

#[test]
fn update_account_no_profile_change_skips_sts() {
    let _sb = Sandbox::new("update-no-sts");
    storage::insert(&seed(
        "dev",
        "dev-profile",
        Environment::Dev,
        "111122223333",
    ))
    .unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Profile is identical → no STS call. If it ever does call STS this
    // would surface as Verification(_) because AWS_CONFIG_FILE points at an
    // empty sandbox directory.
    let updated = rt
        .block_on(accounts::update_account(UpdateAccountInput {
            aws_account_id: "111122223333".to_string(),
            label: "dev-renamed".to_string(),
            profile_name: "dev-profile".to_string(),
            environment: Environment::Staging,
        }))
        .unwrap();
    assert_eq!(updated.label, "dev-renamed");
    assert_eq!(updated.environment, Environment::Staging);
}
