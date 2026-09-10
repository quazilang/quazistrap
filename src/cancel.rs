// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Cooperative cancellation for compiler-internal long-running work.
//!
//! This intentionally has no async-runtime dependency. Front ends may retain
//! a token and request cancellation, while compiler loops periodically call
//! [`CancellationToken::check`] and return [`Cancelled`] without producing a
//! source diagnostic or publishing a partial analysis result.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    requested: Arc<AtomicBool>,
}

/// Cancels an operation unless explicitly disarmed.
///
/// This is useful when an asynchronous caller hands a token to blocking work:
/// dropping the caller's future then reliably notifies the worker without
/// requiring an async runtime in the compiler core.
#[must_use]
pub struct CancelOnDrop {
    token: CancellationToken,
    armed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("compiler operation cancelled")
    }
}

impl std::error::Error for Cancelled {}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), Cancelled> {
        (!self.is_cancelled()).then_some(()).ok_or(Cancelled)
    }

    /// Return a guard that requests cancellation when it is dropped.
    pub fn cancel_on_drop(&self) -> CancelOnDrop {
        CancelOnDrop {
            token: self.clone(),
            armed: true,
        }
    }
}

impl CancelOnDrop {
    /// Keep the operation live after its owning future completed normally.
    pub fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CancellationToken, Cancelled};

    #[test]
    fn cancellation_is_shared_by_token_clones() {
        let first = CancellationToken::new();
        let second = first.clone();

        assert_eq!(first.check(), Ok(()));
        second.cancel();
        assert!(first.is_cancelled());
        assert_eq!(first.check(), Err(Cancelled));
    }

    #[test]
    fn drop_guard_requests_cancellation_unless_disarmed() {
        let cancelled = CancellationToken::new();
        drop(cancelled.cancel_on_drop());
        assert!(cancelled.is_cancelled());

        let completed = CancellationToken::new();
        completed.cancel_on_drop().disarm();
        assert!(!completed.is_cancelled());
    }
}
