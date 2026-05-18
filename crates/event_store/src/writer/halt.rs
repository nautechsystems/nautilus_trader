// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Fail-stop signaling for the event store writer.
//!
//! The kernel registers a [`HaltCallback`] when constructing the writer. The writer invokes
//! the callback once on the first unrecoverable condition: a submit-side backpressure stall
//! that exceeded the configured threshold, or a backend error that breaks the audit
//! contract (disk pressure, corruption, surface I/O failure). The callback is the kernel's
//! signal to fail-stop trading; it is fired exactly once before the writer ceases to accept
//! further entries.

use std::{sync::Arc, time::Duration};

use crate::error::EventStoreError;

/// Reason a writer requested kernel halt.
#[derive(Clone, Debug)]
pub enum HaltReason {
    /// A submit blocked longer than the configured halt threshold while waiting for the
    /// writer thread to drain the channel.
    BackpressureStall {
        /// How long the submit blocked before signaling halt.
        stalled_for: Duration,
        /// The configured threshold the stall exceeded.
        threshold: Duration,
    },
    /// The backend rejected a commit because of disk pressure (ENOSPC, `RLIMIT_FSIZE`,
    /// quota). The audit contract requires fail-stop rather than dropping entries.
    BackendDisk(String),
    /// The backend reported structural corruption.
    BackendCorrupted(String),
    /// The backend returned an unclassified error that the writer cannot retry past.
    BackendError(String),
}

impl HaltReason {
    /// Maps a backend [`EventStoreError`] onto the matching halt reason.
    #[must_use]
    pub fn from_backend_error(err: &EventStoreError) -> Self {
        match err {
            EventStoreError::Disk(msg) => Self::BackendDisk(msg.clone()),
            EventStoreError::Corrupted(msg) => Self::BackendCorrupted(msg.clone()),
            other => Self::BackendError(other.to_string()),
        }
    }
}

/// Callback invoked once on the first unrecoverable writer condition.
///
/// Cloneable so submit, the writer thread, and tests can share the same fail-stop sink.
pub type HaltCallback = Arc<dyn Fn(HaltReason) + Send + Sync + 'static>;

/// Returns a [`HaltCallback`] that performs no action.
///
/// Useful for tests and for writers operating under simulation where halt is observed
/// via the test harness rather than the kernel.
#[must_use]
pub fn noop_halt() -> HaltCallback {
    Arc::new(|_reason| ())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn from_backend_error_classifies_disk_as_disk() {
        let err = EventStoreError::Disk("ENOSPC".to_string());
        let reason = HaltReason::from_backend_error(&err);

        match reason {
            HaltReason::BackendDisk(msg) => assert!(msg.contains("ENOSPC"), "msg was: {msg}"),
            other => panic!("expected BackendDisk, was {other:?}"),
        }
    }

    #[rstest]
    fn from_backend_error_classifies_corrupted_as_corrupted() {
        let err = EventStoreError::Corrupted("bad page".to_string());
        let reason = HaltReason::from_backend_error(&err);

        match reason {
            HaltReason::BackendCorrupted(msg) => {
                assert!(msg.contains("bad page"), "msg was: {msg}");
            }
            other => panic!("expected BackendCorrupted, was {other:?}"),
        }
    }

    #[rstest]
    fn from_backend_error_classifies_other_variants_as_error() {
        let err = EventStoreError::Closed;
        let reason = HaltReason::from_backend_error(&err);

        match reason {
            HaltReason::BackendError(_) => {}
            other => panic!("expected BackendError, was {other:?}"),
        }
    }

    #[rstest]
    fn noop_halt_does_not_panic() {
        let halt = noop_halt();
        halt(HaltReason::BackendDisk("test".to_string()));
    }

    #[rstest]
    fn callback_runs_on_invocation() {
        let captured: Arc<Mutex<Option<HaltReason>>> = Arc::new(Mutex::new(None));
        let captured_for_cb = Arc::clone(&captured);
        let halt: HaltCallback = Arc::new(move |reason| {
            *captured_for_cb.lock().expect("lock") = Some(reason);
        });

        halt(HaltReason::BackendDisk("stall".to_string()));

        match captured.lock().expect("lock").take() {
            Some(HaltReason::BackendDisk(msg)) => assert_eq!(msg, "stall"),
            other => panic!("expected BackendDisk, was {other:?}"),
        }
    }
}
