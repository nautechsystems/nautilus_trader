// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

    #[inline]
    pub fn from_atomic(value: &AtomicU8) -> Self {
        Self::from_u8(value.load(Ordering::SeqCst))
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
