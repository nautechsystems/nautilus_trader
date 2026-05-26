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

//! Cancel commands and their boundary-owned handles.
//!
//! The plug-in constructs a [`CancelOrderCommand`], [`CancelOrdersCommand`],
//! or [`CancelAllOrdersCommand`], wraps it in the matching `*Handle`, and
//! hands the host a `*const XHandle` via the corresponding `HostVTable`
//! slot. The host derefs the handle once and routes the borrowed command
//! into the calling strategy's cancel path. The plug-in owns the box and
//! frees it when the call returns.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_core::Params;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientId, ClientOrderId, InstrumentId},
};

/// Cancel-order command. Mirrors the arguments to `Strategy::cancel_order`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelOrderCommand {
    /// The client order identifier of the order to cancel.
    pub client_order_id: ClientOrderId,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl CancelOrderCommand {
    /// Creates a new [`CancelOrderCommand`] instance.
    #[must_use]
    pub const fn new(
        client_order_id: ClientOrderId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_order_id,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`CancelOrderCommand`] cross the cdylib
/// FFI boundary by reference.
///
/// The plug-in constructs an instance, hands a
/// `*const CancelOrderHandle` to the host for the duration of the
/// `cancel_order` call, and drops the handle when the call returns. The
/// host only borrows the handle and never owns it.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CancelOrderHandle(Box<CancelOrderCommand>);

impl CancelOrderHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: CancelOrderCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &CancelOrderCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> CancelOrderCommand {
        *self.0
    }
}

impl Deref for CancelOrderHandle {
    type Target = CancelOrderCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Cancel-orders (batched) command. Mirrors the arguments to `Strategy::cancel_orders`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelOrdersCommand {
    /// The client order identifiers of the orders to cancel.
    pub client_order_ids: Vec<ClientOrderId>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl CancelOrdersCommand {
    /// Creates a new [`CancelOrdersCommand`] instance.
    #[must_use]
    pub const fn new(
        client_order_ids: Vec<ClientOrderId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_order_ids,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`CancelOrdersCommand`] cross the cdylib
/// FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CancelOrdersHandle(Box<CancelOrdersCommand>);

impl CancelOrdersHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: CancelOrdersCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &CancelOrdersCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> CancelOrdersCommand {
        *self.0
    }
}

impl Deref for CancelOrdersHandle {
    type Target = CancelOrdersCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Cancel-all-orders command. Mirrors the arguments to `Strategy::cancel_all_orders`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelAllOrdersCommand {
    /// The instrument identifier filtering which orders to cancel.
    pub instrument_id: InstrumentId,

    /// Optional order side filter.
    pub order_side: Option<OrderSide>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl CancelAllOrdersCommand {
    /// Creates a new [`CancelAllOrdersCommand`] instance.
    #[must_use]
    pub const fn new(
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            order_side,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`CancelAllOrdersCommand`] cross the
/// cdylib FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CancelAllOrdersHandle(Box<CancelAllOrdersCommand>);

impl CancelAllOrdersHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: CancelAllOrdersCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &CancelAllOrdersCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> CancelAllOrdersCommand {
        *self.0
    }
}

impl Deref for CancelAllOrdersHandle {
    type Target = CancelAllOrdersCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderSide,
        identifiers::{ClientOrderId, InstrumentId},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn cancel_order_handle_round_trips_command() {
        let cmd = CancelOrderCommand::new(ClientOrderId::from("O-1"), None, None);
        let handle = CancelOrderHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }

    #[rstest]
    fn cancel_orders_handle_round_trips_command() {
        let cmd = CancelOrdersCommand::new(
            vec![ClientOrderId::from("O-1"), ClientOrderId::from("O-2")],
            None,
            None,
        );
        let handle = CancelOrdersHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }

    #[rstest]
    fn cancel_all_orders_handle_round_trips_command() {
        let cmd = CancelAllOrdersCommand::new(
            InstrumentId::from("ETH-USDT.BINANCE"),
            Some(OrderSide::Buy),
            None,
            None,
        );
        let handle = CancelAllOrdersHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }
}
