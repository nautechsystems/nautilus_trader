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

//! Shared connection state for socket clients.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use strum::{AsRefStr, Display, EnumString};

/// Irreversible validity token for a single connection's read task.
#[derive(Clone, Debug)]
pub(crate) struct ReadSessionFence {
    valid: Arc<AtomicBool>,
}

impl ReadSessionFence {
    /// Creates a valid fence for a newly spawned read task.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            valid: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Invalidates the associated read session.
    pub(crate) fn invalidate(&self) {
        self.valid.store(false, Ordering::SeqCst);
    }

    /// Returns whether the associated read session is still current.
    #[must_use]
    pub(crate) fn is_valid(&self) -> bool {
        self.valid.load(Ordering::SeqCst)
    }
}

/// Result of a reconnection attempt that did not fail outright.
///
/// A reconnect can finish without reconnecting: a teardown may be requested while
/// it is in flight, at which point it unwinds and leaves the mode terminal. That is
/// normal control flow rather than an error, so it needs to be distinguishable from
/// a completed reconnection by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReconnectOutcome {
    /// A replacement connection was established and the mode is now `Active`.
    Reconnected,
    /// The attempt unwound without reconnecting; the mode was left unchanged.
    Aborted,
}

/// The lifecycle state of a socket client.
///
/// Clients store the active, reconnecting, disconnecting, or closed state in an atomic flag so
/// transport tasks can coordinate lifecycle transitions across threads.
#[derive(Clone, Copy, Debug, Default, Display, Hash, PartialEq, Eq, AsRefStr, EnumString)]
#[repr(u8)]
#[strum(serialize_all = "UPPERCASE")]
pub enum ConnectionMode {
    #[default]
    /// The client is fully connected and operational.
    /// All tasks (reading, writing, heartbeat) are running normally.
    Active = 0,
    /// The client has been disconnected or has been explicitly signaled to reconnect.
    /// In this state, active tasks are paused until a new connection is established.
    Reconnect = 1,
    /// The client has been explicitly signaled to disconnect.
    /// No further reconnection attempts will be made, and cleanup procedures are initiated.
    Disconnect = 2,
    /// The client is permanently closed.
    /// All associated tasks have been terminated and the connection is no longer available.
    Closed = 3,
}

impl ConnectionMode {
    /// Converts a `u8` loaded from an [`AtomicU8`] into a [`ConnectionMode`].
    ///
    /// # Panics
    ///
    /// Panics if `value` is not a valid `ConnectionMode` discriminant (must be between 0 and 3 inclusive).
    #[inline]
    #[must_use]
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Active,
            1 => Self::Reconnect,
            2 => Self::Disconnect,
            3 => Self::Closed,
            _ => panic!("Invalid `ConnectionMode` value: {value}"),
        }
    }

    /// Loads a [`ConnectionMode`] from an [`AtomicU8`] using sequential consistency.
    #[inline]
    #[must_use]
    pub fn from_atomic(value: &AtomicU8) -> Self {
        Self::from_u8(value.load(Ordering::SeqCst))
    }

    /// Atomically transitions to `Reconnect`, but only from `Active`.
    ///
    /// Returns `true` if this call performed the transition. A concurrent
    /// `Disconnect`/`Closed` (or an in-flight `Reconnect`) is left untouched,
    /// so a writer detecting a dead connection cannot resurrect a client that
    /// is being torn down.
    pub fn request_reconnect(value: &AtomicU8) -> bool {
        value
            .compare_exchange(
                Self::Active.as_u8(),
                Self::Reconnect.as_u8(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Atomically transitions to `Disconnect` from any non-`Closed` state.
    ///
    /// Returns `true` if the mode is now `Disconnect`; `false` if the
    /// connection was already `Closed` (terminal state is preserved so status
    /// queries keep reporting `Closed`).
    pub fn request_disconnect(value: &AtomicU8) -> bool {
        value
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |mode| {
                (!Self::from_u8(mode).is_closed()).then_some(Self::Disconnect.as_u8())
            })
            .is_ok()
    }

    /// Atomically completes a reconnection by transitioning `Reconnect` to `Active`.
    ///
    /// Returns [`ReconnectOutcome::Reconnected`] if this call performed the
    /// transition, and [`ReconnectOutcome::Aborted`] if the mode had already moved
    /// on - which a concurrent [`Self::request_disconnect`] or a terminal `Closed`
    /// store can do while the reconnect is in flight. Aborting here is not an
    /// error: it means a teardown won the race and the replacement connection must
    /// not be adopted.
    pub(crate) fn complete_reconnect(value: &AtomicU8) -> ReconnectOutcome {
        if value
            .compare_exchange(
                Self::Reconnect.as_u8(),
                Self::Active.as_u8(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            ReconnectOutcome::Reconnected
        } else {
            ReconnectOutcome::Aborted
        }
    }

    /// Converts a [`ConnectionMode`] to its `u8` representation.
    #[inline]
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns true if the client is in an active state.
    #[inline]
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns true if the client is attempting to reconnect.
    #[inline]
    #[must_use]
    pub const fn is_reconnect(&self) -> bool {
        matches!(self, Self::Reconnect)
    }

    /// Returns true if the client is attempting to disconnect.
    #[inline]
    #[must_use]
    pub const fn is_disconnect(&self) -> bool {
        matches!(self, Self::Disconnect)
    }

    /// Returns true if the client connection is closed.
    #[inline]
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(ConnectionMode::Active, true, ConnectionMode::Reconnect)]
    #[case(ConnectionMode::Reconnect, false, ConnectionMode::Reconnect)]
    #[case(ConnectionMode::Disconnect, false, ConnectionMode::Disconnect)]
    #[case(ConnectionMode::Closed, false, ConnectionMode::Closed)]
    fn request_reconnect_transitions(
        #[case] start: ConnectionMode,
        #[case] expected_result: bool,
        #[case] expected_mode: ConnectionMode,
    ) {
        let mode = AtomicU8::new(start.as_u8());

        assert_eq!(ConnectionMode::request_reconnect(&mode), expected_result);
        assert_eq!(ConnectionMode::from_atomic(&mode), expected_mode);
    }

    // Only a mode still in `Reconnect` may be adopted as `Active`. A teardown that won
    // the race - `Disconnect` or `Closed` - must report `Aborted` and leave the terminal
    // mode intact, so the caller can tell a completed reconnection from an unwound one.
    #[rstest]
    #[case(
        ConnectionMode::Reconnect,
        ReconnectOutcome::Reconnected,
        ConnectionMode::Active
    )]
    #[case(
        ConnectionMode::Disconnect,
        ReconnectOutcome::Aborted,
        ConnectionMode::Disconnect
    )]
    #[case(
        ConnectionMode::Closed,
        ReconnectOutcome::Aborted,
        ConnectionMode::Closed
    )]
    #[case(
        ConnectionMode::Active,
        ReconnectOutcome::Aborted,
        ConnectionMode::Active
    )]
    fn complete_reconnect_transitions(
        #[case] start: ConnectionMode,
        #[case] expected_outcome: ReconnectOutcome,
        #[case] expected_mode: ConnectionMode,
    ) {
        let mode = AtomicU8::new(start.as_u8());

        assert_eq!(ConnectionMode::complete_reconnect(&mode), expected_outcome);
        assert_eq!(ConnectionMode::from_atomic(&mode), expected_mode);
    }

    #[rstest]
    #[case(ConnectionMode::Active, true, ConnectionMode::Disconnect)]
    #[case(ConnectionMode::Reconnect, true, ConnectionMode::Disconnect)]
    #[case(ConnectionMode::Disconnect, true, ConnectionMode::Disconnect)]
    #[case(ConnectionMode::Closed, false, ConnectionMode::Closed)]
    fn request_disconnect_transitions(
        #[case] start: ConnectionMode,
        #[case] expected_result: bool,
        #[case] expected_mode: ConnectionMode,
    ) {
        let mode = AtomicU8::new(start.as_u8());

        assert_eq!(ConnectionMode::request_disconnect(&mode), expected_result);
        assert_eq!(ConnectionMode::from_atomic(&mode), expected_mode);
    }
}
