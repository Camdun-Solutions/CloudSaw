// Integration tests for the app-lock module (Contract 02 + 02-QA).
//
// These exercise the *real* public surface of `applock` against a real
// SQLite database in a temp directory. They cover every Happy Path,
// Error State, and State Transition item from C02-app-lock-QA.md that can
// be verified without a live OS biometric or identity prompt.
//
// Tests are serialized through a module-level mutex because they share the
// `CLOUDSAW_DATA_DIR_OVERRIDE` env var — running them in parallel would race.
// Each test claims the mutex, points the override at a fresh tempdir,
// initializes the schema, and tears down on drop.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use zeroize::Zeroizing;

use cloudsaw_lib::applock::{self, LockPeriod, LockSettings, SessionState};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::errors::AppError;

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        // Poisoning is fine — a panicking test in another iteration shouldn't
        // stop subsequent tests from claiming the env var.
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-applock-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        // Apply migrations so app_lock exists.
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();
        Self { _guard: guard, dir }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("db").join("cloudsaw.db")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn pw(s: &str) -> Zeroizing<String> {
    Zeroizing::new(s.to_string())
}

// --- Happy Path ----------------------------------------------------------

#[test]
fn first_run_set_password_unlocks_and_persists() {
    let sb = Sandbox::new("firstrun");
    let session = SessionState::new();

    let state = applock::get_state(&session).unwrap();
    assert!(state.first_run, "fresh DB must report first_run = true");
    assert!(
        !state.locked,
        "first_run takes priority over locked in the gate logic"
    );

    applock::set_master_password(&session, pw("correctpassword")).unwrap();
    assert!(session.is_unlocked());

    let state = applock::get_state(&session).unwrap();
    assert!(!state.first_run);
    assert!(!state.locked);

    // The PHC string is stored, and it is in fact Argon2id (not plaintext).
    let conn = Connection::open(sb.db_path()).unwrap();
    let stored: String = conn
        .query_row("SELECT password_hash FROM app_lock WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(
        stored.starts_with("$argon2id$"),
        "stored value must be a PHC argon2id string, got: {stored}"
    );
    assert!(
        !stored.contains("correctpassword"),
        "plaintext password must never appear in the stored hash"
    );
}

#[test]
fn correct_password_unlocks_and_wrong_does_not() {
    let _sb = Sandbox::new("unlock");
    let session = SessionState::new();
    applock::set_master_password(&session, pw("correctpassword")).unwrap();

    // Simulate "app restart" by minting a new SessionState. Storage outlives
    // the process; SessionState does not.
    let session = SessionState::new();
    assert!(applock::is_locked(&session));

    // Wrong password.
    let err = applock::unlock(&session, pw("wrongpassword")).unwrap_err();
    assert!(matches!(err, AppError::PasswordRejected));
    assert!(applock::is_locked(&session));

    // Correct password.
    applock::unlock(&session, pw("correctpassword")).unwrap();
    assert!(!applock::is_locked(&session));
}

#[test]
fn timed_period_keeps_session_unlocked_across_app_restarts() {
    let _sb = Sandbox::new("timed");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    // Default period is 7 days; an immediate "restart" should bootstrap to
    // unlocked because last_unlocked_at was just written.
    let restored = applock::bootstrap_session().unwrap();
    assert!(!applock::is_locked(&restored));
}

#[test]
fn immediate_on_close_relocks_on_every_launch() {
    let _sb = Sandbox::new("immediate");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    applock::set_lock_settings(LockSettings {
        lock_period: LockPeriod::Immediate,
        biometric_enabled: false,
    })
    .unwrap();
    let restored = applock::bootstrap_session().unwrap();
    assert!(
        applock::is_locked(&restored),
        "immediate-on-close MUST re-prompt on next launch"
    );
}

#[test]
fn never_relock_stays_unlocked_after_first_unlock() {
    let _sb = Sandbox::new("never");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    applock::set_lock_settings(LockSettings {
        lock_period: LockPeriod::Never,
        biometric_enabled: false,
    })
    .unwrap();
    let restored = applock::bootstrap_session().unwrap();
    assert!(
        !applock::is_locked(&restored),
        "never-relock should not re-prompt once the user has unlocked"
    );
}

#[test]
fn lock_settings_round_trip() {
    let _sb = Sandbox::new("settings");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();

    let configured = LockSettings {
        lock_period: LockPeriod::After(86_400),
        biometric_enabled: false,
    };
    applock::set_lock_settings(configured.clone()).unwrap();

    let readback = applock::get_lock_settings().unwrap();
    assert!(matches!(readback.lock_period, LockPeriod::After(86_400)));
    assert!(!readback.biometric_enabled);
}

#[test]
fn change_password_succeeds_with_correct_old() {
    let _sb = Sandbox::new("change_ok");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("originalpw")).unwrap();
    applock::change_password(&s, pw("originalpw"), pw("brandnewpw")).unwrap();
    assert!(applock::verify_password(pw("brandnewpw")).unwrap());
    assert!(!applock::verify_password(pw("originalpw")).unwrap());
}

// --- Error States --------------------------------------------------------

#[test]
fn incorrect_password_gives_binary_failure_no_closeness_hint() {
    let _sb = Sandbox::new("binary");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();

    // "Close" and "totally wrong" must surface as the *same* error variant.
    let close = applock::unlock(&SessionState::new(), pw("correctpasswor")).unwrap_err();
    let far = applock::unlock(&SessionState::new(), pw("zzzzzzzz")).unwrap_err();
    assert!(matches!(close, AppError::PasswordRejected));
    assert!(matches!(far, AppError::PasswordRejected));
    // And the stable code field matches too — that's what the UI keys on.
    assert_eq!(close.code(), far.code());
}

#[test]
fn change_password_with_wrong_old_leaves_old_password_valid() {
    let _sb = Sandbox::new("change_bad");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("originalpw")).unwrap();

    let err = applock::change_password(&s, pw("WRONGOLD"), pw("brandnewpw")).unwrap_err();
    assert!(matches!(err, AppError::PasswordRejected));

    // Critical invariant from Contract 02 edge cases: a failed change leaves
    // the old password still valid.
    assert!(applock::verify_password(pw("originalpw")).unwrap());
    assert!(!applock::verify_password(pw("brandnewpw")).unwrap());
}

#[test]
fn first_run_setup_cannot_be_re_done() {
    let _sb = Sandbox::new("dup");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("originalpw")).unwrap();
    let err = applock::set_master_password(&s, pw("anothernewpw")).unwrap_err();
    assert!(matches!(err, AppError::AlreadyConfigured));
    assert!(applock::verify_password(pw("originalpw")).unwrap());
}

#[test]
fn enabling_biometric_when_unavailable_is_refused() {
    // The platform implementations on macOS/Linux return Unavailable, and the
    // Windows test runner here may or may not have Hello configured. Either
    // way, an Unavailable platform MUST refuse the setting — the test is
    // skipped on a host where biometric actually is Available.
    let _sb = Sandbox::new("bio");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();

    let availability = applock::biometric::availability();
    if matches!(availability, applock::biometric::Availability::Available) {
        eprintln!("skipped: host reports biometric Available, can't exercise refusal");
        return;
    }
    let err = applock::set_lock_settings(LockSettings {
        lock_period: LockPeriod::After(86_400),
        biometric_enabled: true,
    })
    .unwrap_err();
    assert!(matches!(err, AppError::BiometricUnavailable));
}

#[test]
fn unlock_before_setup_returns_not_configured() {
    let _sb = Sandbox::new("notcfg");
    let s = SessionState::new();
    let err = applock::unlock(&s, pw("anything1")).unwrap_err();
    assert!(matches!(err, AppError::NotConfigured));
}

#[test]
fn rapid_failed_attempts_arm_backoff_and_never_lock_out_permanently() {
    let _sb = Sandbox::new("backoff");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    let s = SessionState::new();

    // First three wrong attempts return PasswordRejected without backoff.
    for _ in 0..3 {
        let e = applock::unlock(&s, pw("wrongone")).unwrap_err();
        assert!(matches!(e, AppError::PasswordRejected));
    }
    // From the 4th, backoff arms. Subsequent attempts return RateLimited
    // with a bounded delay.
    let mut saw_rate_limit = false;
    let mut max_delay_seen = 0u64;
    for _ in 0..20 {
        match applock::unlock(&s, pw("wrongtwo")) {
            Err(AppError::RateLimited(d)) => {
                saw_rate_limit = true;
                max_delay_seen = max_delay_seen.max(d);
                assert!(d <= 60, "backoff must cap at 60s, saw {d}");
            }
            Err(AppError::PasswordRejected) => {}
            other => panic!("unexpected unlock result during backoff probe: {other:?}"),
        }
    }
    assert!(saw_rate_limit, "backoff MUST eventually fire");
    assert!(
        max_delay_seen > 0,
        "backoff delay should be positive once armed"
    );

    // "Never permanent" — the rate limit MUST be a finite wait, not a flag.
    // We can't sleep the full backoff in a unit test, so verify the property
    // structurally: the session API exposes no "permanently locked" sink, and
    // we just confirmed every error variant we saw was Rejected or
    // RateLimited(finite seconds).
}

#[test]
fn rate_limited_unlock_does_not_succeed_until_wait_elapses() {
    let _sb = Sandbox::new("waitbackoff");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    let s = SessionState::new();

    // Burn enough wrong attempts to arm a 1s window.
    let mut wait_secs = 0u64;
    for _ in 0..8 {
        if let Err(AppError::RateLimited(d)) = applock::unlock(&s, pw("wrongthree")) {
            wait_secs = d;
            break;
        }
    }
    assert!(wait_secs >= 1, "expected at least 1s wait, got {wait_secs}");

    // Right now, even the *correct* password must be rate-limited.
    let immediate = applock::unlock(&s, pw("correctpassword"));
    assert!(matches!(immediate, Err(AppError::RateLimited(_))));

    // Sleep just past the smallest backoff window and try again.
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(1100));
    assert!(started.elapsed() >= Duration::from_secs(1));
    applock::unlock(&s, pw("correctpassword")).unwrap();
    assert!(!applock::is_locked(&s));
}

#[test]
fn successful_unlock_clears_backoff() {
    let _sb = Sandbox::new("clearbo");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    let s = SessionState::new();

    // Try a few wrong ones, then one right one, then another wrong one.
    for _ in 0..3 {
        let _ = applock::unlock(&s, pw("wrongfour"));
    }
    applock::unlock(&s, pw("correctpassword")).unwrap();
    // Lock manually and try one wrong; we should be back at attempt-count 1,
    // i.e. PasswordRejected, not RateLimited.
    applock::lock(&s);
    let err = applock::unlock(&s, pw("wrongfive")).unwrap_err();
    assert!(matches!(err, AppError::PasswordRejected));
}

// --- State Transitions ---------------------------------------------------

#[test]
fn manual_lock_then_unlock_round_trip() {
    let _sb = Sandbox::new("manual");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("correctpassword")).unwrap();
    assert!(!applock::is_locked(&s));
    applock::lock(&s);
    assert!(applock::is_locked(&s));
    applock::unlock(&s, pw("correctpassword")).unwrap();
    assert!(!applock::is_locked(&s));
}

#[test]
fn password_change_keeps_session_unlocked_with_new_password() {
    let _sb = Sandbox::new("changekeep");
    let s = SessionState::new();
    applock::set_master_password(&s, pw("originalpw")).unwrap();
    applock::change_password(&s, pw("originalpw"), pw("brandnewpw")).unwrap();
    assert!(
        !applock::is_locked(&s),
        "change-password should keep the session unlocked"
    );
    // The new password is now the one that works on a future unlock.
    applock::lock(&s);
    applock::unlock(&s, pw("brandnewpw")).unwrap();
}
