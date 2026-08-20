//! Trusted time boundary for immutable Agent OS governance evidence.
//!
//! Governance services obtain final evidence timestamps through this contract.
//! Request timestamps may still be supplied by callers where an existing domain
//! requires them, but snapshots, decisions, audit events, and boundary evidence
//! must use a trusted clock implementation.

use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TrustedClockError {
    #[error("Trusted clock produced a timestamp before the Unix epoch")]
    BeforeUnixEpoch,
    #[error("Trusted clock timestamp exceeds the supported range")]
    OutOfRange,
    #[error("Trusted clock timestamp must not be negative")]
    InvalidTimestamp,
}

pub trait TrustedClock: Send + Sync {
    fn now(&self) -> Result<i64, TrustedClockError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTrustedClock;

impl TrustedClock for SystemTrustedClock {
    fn now(&self) -> Result<i64, TrustedClockError> {
        let milliseconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TrustedClockError::BeforeUnixEpoch)?
            .as_millis();
        i64::try_from(milliseconds).map_err(|_| TrustedClockError::OutOfRange)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedTrustedClock {
    timestamp: i64,
}

impl FixedTrustedClock {
    pub fn new(timestamp: i64) -> Result<Self, TrustedClockError> {
        if timestamp < 0 {
            return Err(TrustedClockError::InvalidTimestamp);
        }
        Ok(Self { timestamp })
    }
}

impl TrustedClock for FixedTrustedClock {
    fn now(&self) -> Result<i64, TrustedClockError> {
        Ok(self.timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_rejects_forged_negative_time() {
        assert_eq!(
            FixedTrustedClock::new(-1),
            Err(TrustedClockError::InvalidTimestamp)
        );
    }

    #[test]
    fn system_clock_uses_unix_milliseconds() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let observed = SystemTrustedClock.now().unwrap();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        assert!((before..=after).contains(&observed));
        assert!(observed > 1_000_000_000_000);
    }
}
