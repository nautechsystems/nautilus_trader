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

//! Close-position and close-all-positions commands and their boundary-owned
//! handles.
//!
//! The plug-in constructs a [`ClosePositionCommand`] or
//! [`CloseAllPositionsCommand`], wraps it in the matching `*Handle`, and
//! hands the host a pointer via
//! [`HostVTable::close_position`](crate::host::HostVTable::close_position) or
//! [`HostVTable::close_all_positions`](crate::host::HostVTable::close_all_positions).
//! The host derefs the handle once and routes the borrowed command into
//! the calling strategy's close path. The plug-in owns the box and frees
//! it when the call returns.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_model::{
    enums::{PositionSide, TimeInForce},
    identifiers::{ClientId, InstrumentId, PositionId},
};
use ustr::Ustr;

/// Close-position command. Mirrors the arguments to `Strategy::close_position`.
///
/// The host resolves `position_id` against the live cache to materialise the
/// `&Position` reference the trait method requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosePositionCommand {
    /// The identifier of the position to close.
    pub position_id: PositionId,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional tags to attach to the closing order.
    pub tags: Option<Vec<Ustr>>,

    /// Optional time-in-force override.
    pub time_in_force: Option<TimeInForce>,

    /// Optional reduce-only flag override.
    pub reduce_only: Option<bool>,

    /// Optional quote-quantity flag override.
    pub quote_quantity: Option<bool>,
}

impl ClosePositionCommand {
    /// Creates a new [`ClosePositionCommand`] instance.
    #[must_use]
    pub const fn new(
        position_id: PositionId,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> Self {
        Self {
            position_id,
            client_id,
            tags,
            time_in_force,
            reduce_only,
            quote_quantity,
        }
    }
}

/// Boundary-owned wrapper that lets [`ClosePositionCommand`] cross the cdylib
/// FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ClosePositionHandle(Box<ClosePositionCommand>);

impl ClosePositionHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: ClosePositionCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &ClosePositionCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> ClosePositionCommand {
        *self.0
    }
}

impl Deref for ClosePositionHandle {
    type Target = ClosePositionCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Close-all-positions command. Mirrors the arguments to `Strategy::close_all_positions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseAllPositionsCommand {
    /// The instrument identifier filtering which positions to close.
    pub instrument_id: InstrumentId,

    /// Optional position side filter.
    pub position_side: Option<PositionSide>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional tags to attach to the closing orders.
    pub tags: Option<Vec<Ustr>>,

    /// Optional time-in-force override.
    pub time_in_force: Option<TimeInForce>,

    /// Optional reduce-only flag override.
    pub reduce_only: Option<bool>,

    /// Optional quote-quantity flag override.
    pub quote_quantity: Option<bool>,
}

impl CloseAllPositionsCommand {
    /// Creates a new [`CloseAllPositionsCommand`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        instrument_id: InstrumentId,
        position_side: Option<PositionSide>,
        client_id: Option<ClientId>,
        tags: Option<Vec<Ustr>>,
        time_in_force: Option<TimeInForce>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
    ) -> Self {
        Self {
            instrument_id,
            position_side,
            client_id,
            tags,
            time_in_force,
            reduce_only,
            quote_quantity,
        }
    }
}

/// Boundary-owned wrapper that lets [`CloseAllPositionsCommand`] cross the
/// cdylib FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CloseAllPositionsHandle(Box<CloseAllPositionsCommand>);

impl CloseAllPositionsHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: CloseAllPositionsCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &CloseAllPositionsCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> CloseAllPositionsCommand {
        *self.0
    }
}

impl Deref for CloseAllPositionsHandle {
    type Target = CloseAllPositionsCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn close_position_handle_round_trips_command() {
        let cmd = ClosePositionCommand::new(
            PositionId::from("P-001"),
            None,
            Some(vec![Ustr::from("exit")]),
            Some(TimeInForce::Ioc),
            None,
            None,
        );
        let handle = ClosePositionHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }

    #[rstest]
    fn close_all_positions_handle_round_trips_command() {
        let cmd = CloseAllPositionsCommand::new(
            InstrumentId::from("ETH-USDT.BINANCE"),
            Some(PositionSide::Long),
            None,
            None,
            None,
            None,
            None,
        );
        let handle = CloseAllPositionsHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }
}
