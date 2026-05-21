// In-memory session state for the app lock.
//
// Tracks the *current process's* notion of "is the app unlocked right now" and
// applies a backoff to failed unlock attempts. Both live behind a single
// Mutex; contention is irrelevant in practice (lock operations happen at
// human speed).
//
// Contract 02 edge case: "Many rapid failed unlock attempts → attempts are
// rate-limited/back-off applied; the app does not crash or lock out
// permanently." We implement that with exponential backoff that caps at 60s
// and resets to zero after any successful unlock.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::errors::AppError;

#[derive(Debug, Default)]
struct Inner {
    unlocked: bool,
    failed_attempts: u32,
    backoff_until: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct SessionState {
    inner: Mutex<Inner>,
}

impl SessionState {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                unlocked: false,
                failed_attempts: 0,
                backoff_until: None,
            }),
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.inner.lock().expect("session state poisoned").unlocked
    }

    pub fn mark_unlocked(&self) {
        let mut g = self.inner.lock().expect("session state poisoned");
        g.unlocked = true;
        g.failed_attempts = 0;
        g.backoff_until = None;
    }

    pub fn mark_locked(&self) {
        let mut g = self.inner.lock().expect("session state poisoned");
        g.unlocked = false;
    }

    /// Returns the remaining backoff if attempts must wait, else `None`.
    pub fn check_backoff(&self) -> Option<Duration> {
        let g = self.inner.lock().expect("session state poisoned");
        match g.backoff_until {
            Some(until) => {
                let now = Instant::now();
                if until > now {
                    Some(until - now)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Record a failed unlock attempt and arm the next backoff window.
    /// Returns the new backoff duration (zero if no wait yet required).
    pub fn record_failure(&self) -> Duration {
        let mut g = self.inner.lock().expect("session state poisoned");
        g.unlocked = false;
        g.failed_attempts = g.failed_attempts.saturating_add(1);
        let delay = backoff_for(g.failed_attempts);
        g.backoff_until = if delay.is_zero() {
            None
        } else {
            Some(Instant::now() + delay)
        };
        delay
    }
}

/// Exponential-ish curve: silent for the first few attempts so a normal typo
/// doesn't stutter the UI, then ramps to a 60s cap. Never permanent.
fn backoff_for(attempts: u32) -> Duration {
    match attempts {
        0..=3 => Duration::ZERO,
        4 => Duration::from_secs(1),
        5 => Duration::from_secs(3),
        6 => Duration::from_secs(10),
        7 => Duration::from_secs(30),
        _ => Duration::from_secs(60),
    }
}

/// Helper that converts a remaining backoff window into the typed error
/// surfaced over IPC.
pub fn backoff_to_error(d: Duration) -> AppError {
    AppError::RateLimited(d.as_secs().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_locked() {
        let s = SessionState::new();
        assert!(!s.is_unlocked());
        assert!(s.check_backoff().is_none());
    }

    #[test]
    fn unlocking_clears_failures() {
        let s = SessionState::new();
        for _ in 0..5 {
            s.record_failure();
        }
        s.mark_unlocked();
        assert!(s.is_unlocked());
        assert!(s.check_backoff().is_none());
    }

    #[test]
    fn backoff_caps_and_never_permanent() {
        let s = SessionState::new();
        for _ in 0..50 {
            let d = s.record_failure();
            assert!(d <= Duration::from_secs(60));
        }
    }
}
