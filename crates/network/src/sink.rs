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

//! Ordered availability-edge publication for socket transports.
//!
//! # Ordering
//!
//! A [`SocketStateSink`] reports transitions into and out of [`ConnectionMode::Active`] for one
//! client, rather than every internal connection mode. Most paths perform the mode transition and
//! callback under the same serialization lock, so concurrent loss and recovery attempts publish
//! at most one ordered edge. WebSocket reconnect changes mode before publishing loss and uses the
//! same lock for publication, while the controller prevents recovery until publication completes.

use std::{
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

use crate::mode::ConnectionMode;

/// Receives ordered semantic state changes from a single socket client.
///
/// Clients must route every transition into or out of [`ConnectionMode::Active`] through the
/// sink-backed transition methods so each edge is reported once.
#[derive(Clone)]
pub struct SocketStateSink {
    callback: Arc<dyn Fn(SocketState) + Send + Sync>,
    transition_lock: Arc<Mutex<()>>,
}

impl SocketStateSink {
    /// Creates a new [`SocketStateSink`] instance.
    ///
    /// The callback runs synchronously with each successful state transition and should return
    /// promptly. It must not initiate another transition using the same sink because callbacks are
    /// serialized under a non-reentrant lock.
    #[must_use]
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(SocketState) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
            transition_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Returns a sink that invokes `callback` before forwarding each state to this sink.
    #[must_use]
    pub fn with_callback<F>(self, callback: F) -> Self
    where
        F: Fn(SocketState) + Send + Sync + 'static,
    {
        let Self {
            callback: forwarded,
            transition_lock,
        } = self;

        Self {
            callback: Arc::new(move |state| {
                callback(state);
                forwarded(state);
            }),
            transition_lock,
        }
    }

    pub(crate) fn transition(
        &self,
        value: &AtomicU8,
        current: ConnectionMode,
        next: ConnectionMode,
        state: SocketState,
    ) -> bool {
        self.transition_result(value, current, next, state).is_ok()
    }

    pub(crate) fn transition_result(
        &self,
        value: &AtomicU8,
        current: ConnectionMode,
        next: ConnectionMode,
        state: SocketState,
    ) -> Result<(), ConnectionMode> {
        let _guard = self
            .transition_lock
            .lock()
            .expect("socket state sink transition lock poisoned");

        if let Err(actual) = value.compare_exchange(
            current.as_u8(),
            next.as_u8(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            return Err(ConnectionMode::from_u8(actual));
        }

        self.notify(state);

        Ok(())
    }

    pub(crate) fn publish_websocket(&self, state: SocketState) {
        let _guard = self
            .transition_lock
            .lock()
            .expect("socket state sink transition lock poisoned");
        self.notify(state);
    }

    pub(crate) fn close_on_loss(&self, value: &AtomicU8) -> bool {
        let _guard = self
            .transition_lock
            .lock()
            .expect("socket state sink transition lock poisoned");
        let current = ConnectionMode::from_atomic(value);

        if !matches!(current, ConnectionMode::Active | ConnectionMode::Reconnect)
            || value
                .compare_exchange(
                    current.as_u8(),
                    ConnectionMode::Closed.as_u8(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_err()
        {
            return false;
        }

        if current.is_active() {
            self.notify(SocketState::Disconnected);
        }

        true
    }

    fn notify(&self, state: SocketState) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.callback)(state)))
            .is_err()
        {
            log::error!("Socket state sink panicked while handling {state:?}");
        }
    }
}

impl Debug for SocketStateSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketStateSink))
            .finish_non_exhaustive()
    }
}

/// Represents the availability state reported by a socket transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SocketState {
    /// The transport is available.
    Connected,
    /// An active transport was lost.
    Disconnected,
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Barrier, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering as AtomicOrdering},
    };

    use rstest::rstest;

    use super::*;
    use crate::mode::{ConnectionMode, ReconnectOutcome};

    #[rstest]
    fn state_sink_reports_only_successful_edges_in_order() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let mode = AtomicU8::new(ConnectionMode::Reconnect.as_u8());

        assert_eq!(
            ConnectionMode::complete_reconnect_with_sink(&mode, Some(&sink)),
            ReconnectOutcome::Reconnected
        );
        assert!(ConnectionMode::request_reconnect_with_sink(
            &mode,
            Some(&sink)
        ));
        assert!(!ConnectionMode::request_reconnect_with_sink(
            &mode,
            Some(&sink)
        ));
        assert_eq!(
            ConnectionMode::complete_reconnect_with_sink(&mode, Some(&sink)),
            ReconnectOutcome::Reconnected
        );

        assert_eq!(
            *states.lock().unwrap(),
            vec![
                SocketState::Connected,
                SocketState::Disconnected,
                SocketState::Connected,
            ]
        );
    }

    #[rstest]
    fn state_sink_with_callback_runs_before_forwarded_sink() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let forwarded_calls = Arc::clone(&calls);
        let sink = SocketStateSink::new(move |_| {
            forwarded_calls.lock().unwrap().push("forwarded");
        });
        let transition_lock = Arc::clone(&sink.transition_lock);
        let callback_calls = Arc::clone(&calls);
        let sink = sink.with_callback(move |_| {
            callback_calls.lock().unwrap().push("callback");
        });
        let mode = AtomicU8::new(ConnectionMode::Reconnect.as_u8());

        assert_eq!(
            ConnectionMode::complete_reconnect_with_sink(&mode, Some(&sink)),
            ReconnectOutcome::Reconnected
        );
        assert!(Arc::ptr_eq(&sink.transition_lock, &transition_lock));
        assert_eq!(*calls.lock().unwrap(), vec!["callback", "forwarded"]);
    }

    #[rstest]
    fn state_sink_reports_one_concurrent_loss() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let barrier = Arc::new(Barrier::new(8));

        let mut transitions = Vec::with_capacity(8);

        for _ in 0..8 {
            let mode = Arc::clone(&mode);
            let sink = sink.clone();
            let barrier = Arc::clone(&barrier);
            transitions.push(std::thread::spawn(move || {
                barrier.wait();
                ConnectionMode::request_reconnect_with_sink(&mode, Some(&sink))
            }));
        }

        let successful = transitions
            .into_iter()
            .map(|transition| transition.join().unwrap())
            .filter(|successful| *successful)
            .count();

        assert_eq!(successful, 1);
        assert_eq!(
            ConnectionMode::from_atomic(&mode),
            ConnectionMode::Reconnect
        );
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
    }

    #[rstest]
    fn state_sink_reports_one_mixed_concurrent_loss() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let barrier = Arc::new(Barrier::new(2));

        let reconnect = {
            let mode = Arc::clone(&mode);
            let sink = sink.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                ConnectionMode::request_reconnect_with_sink(&mode, Some(&sink))
            })
        };

        let close = std::thread::spawn({
            let mode = Arc::clone(&mode);
            move || {
                barrier.wait();
                ConnectionMode::close_websocket_on_loss(&mode, Some(&sink))
            }
        });

        reconnect.join().unwrap();
        let closed = close.join().unwrap();

        assert!(closed);
        assert_eq!(ConnectionMode::from_atomic(&mode), ConnectionMode::Closed);
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
    }

    #[rstest]
    fn state_sink_continues_after_callback_panic() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_callback = Arc::clone(&calls);
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            assert_ne!(
                calls_callback.fetch_add(1, AtomicOrdering::SeqCst),
                0,
                "test socket state callback panic"
            );
            states_callback.lock().unwrap().push(state);
        });
        let mode = AtomicU8::new(ConnectionMode::Reconnect.as_u8());

        assert_eq!(
            ConnectionMode::complete_reconnect_with_sink(&mode, Some(&sink)),
            ReconnectOutcome::Reconnected
        );
        assert!(ConnectionMode::request_reconnect_with_sink(
            &mode,
            Some(&sink)
        ));

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(
            ConnectionMode::from_atomic(&mode),
            ConnectionMode::Reconnect
        );
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
    }

    #[rstest]
    fn state_sink_suppresses_deliberate_disconnect() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let mode = AtomicU8::new(ConnectionMode::Active.as_u8());

        assert!(ConnectionMode::request_disconnect(&mode));
        assert!(!ConnectionMode::request_reconnect_with_sink(
            &mode,
            Some(&sink)
        ));

        assert_eq!(*states.lock().unwrap(), Vec::new());
    }

    #[rstest]
    fn state_sink_closes_after_reported_loss_without_another_event() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let mode = AtomicU8::new(ConnectionMode::Reconnect.as_u8());

        assert!(ConnectionMode::close_websocket_on_loss(&mode, Some(&sink)));

        assert_eq!(ConnectionMode::from_atomic(&mode), ConnectionMode::Closed);
        assert_eq!(*states.lock().unwrap(), Vec::new());
    }
}
