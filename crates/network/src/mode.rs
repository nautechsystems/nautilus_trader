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

//! Connection mode enumeration for socket clients.

use std::sync::atomic::{AtomicU8, Ordering};

use strum::{AsRefStr, Display, EnumString};

/// Connection mode for a socket client.
///
/// The client can be in one of four modes (managed via an atomic flag).
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
    /// Convert a u8 to [`ConnectionMode`], useful when loading from an `AtomicU8`.
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

    /// Load a [`ConnectionMode`] from an [`AtomicU8`] using sequential consistency ordering.
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
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |mode| {
                (!Self::from_u8(mode).is_closed()).then_some(Self::Disconnect.as_u8())
            })
            .is_ok()
    }

    /// Convert a [`ConnectionMode`] to a u8, useful when storing to an `AtomicU8`.
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
