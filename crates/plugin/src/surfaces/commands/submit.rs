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

//! Submit commands and their boundary-owned handles.
//!
//! The plug-in constructs a [`SubmitOrderCommand`] or
//! [`SubmitOrderListCommand`], wraps it in the matching `*Handle`, and
//! hands the host a pointer via
//! [`HostVTable::submit_order`](crate::host::HostVTable::submit_order) or
//! [`HostVTable::submit_order_list`](crate::host::HostVTable::submit_order_list).
//! The host derefs the handle once and routes the borrowed command into
//! the calling strategy's submit path. The plug-in owns the box and frees
//! it when the call returns.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_core::Params;
use nautilus_model::{
    identifiers::{ClientId, PositionId},
    orders::OrderAny,
};

/// Submit-order command. Mirrors the arguments to
/// [`Strategy::submit_order`](nautilus_trading::strategy::Strategy::submit_order).
#[derive(Debug, Clone)]
pub struct SubmitOrderCommand {
    /// The order to submit.
    pub order: OrderAny,

    /// Optional position the order is associated with.
    pub position_id: Option<PositionId>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl SubmitOrderCommand {
    /// Creates a new [`SubmitOrderCommand`] instance.
    #[must_use]
    pub const fn new(
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            order,
            position_id,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`SubmitOrderCommand`] cross the cdylib
/// FFI boundary by reference.
///
/// `SubmitOrderCommand` carries an `OrderAny` whose variant payloads are
/// heap-owned (e.g. tag vectors, exec-algorithm params), so the plug-in
/// wraps the whole command in this `#[repr(C)]` handle and passes a
/// borrowed pointer to the host. Equivalent layout on both sides relies
/// on operator-side pinning (plug-in cdylibs rebuilt to match each
/// Nautilus version); `PluginBuildId` is recorded for load diagnostics
/// but is not enforced by the loader in v1.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SubmitOrderHandle(Box<SubmitOrderCommand>);

impl SubmitOrderHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: SubmitOrderCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &SubmitOrderCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> SubmitOrderCommand {
        *self.0
    }
}

impl Deref for SubmitOrderHandle {
    type Target = SubmitOrderCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Submit-order-list command. Mirrors the arguments to
/// [`Strategy::submit_order_list`](nautilus_trading::strategy::Strategy::submit_order_list).
#[derive(Debug, Clone)]
pub struct SubmitOrderListCommand {
    /// The orders to submit as a batched list.
    pub orders: Vec<OrderAny>,

    /// Optional position the orders are associated with.
    pub position_id: Option<PositionId>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl SubmitOrderListCommand {
    /// Creates a new [`SubmitOrderListCommand`] instance.
    #[must_use]
    pub const fn new(
        orders: Vec<OrderAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            orders,
            position_id,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`SubmitOrderListCommand`] cross the
/// cdylib FFI boundary by reference.
///
/// The `Vec<OrderAny>` payload is the largest of any execution command;
/// the outer `Box` pins the Vec header and the heap allocation stays on
/// the plug-in side for the duration of the call.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SubmitOrderListHandle(Box<SubmitOrderListCommand>);

impl SubmitOrderListHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: SubmitOrderListCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &SubmitOrderListCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> SubmitOrderListCommand {
        *self.0
    }
}

impl Deref for SubmitOrderListHandle {
    type Target = SubmitOrderListCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{MarketOrder, Order},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    fn market_order(client_order_id: &str) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    // SubmitOrderCommand carries an `OrderAny` which is not `Eq`, so identity is
    // asserted via the underlying client_order_id rather than via `==` on the
    // full struct.
    #[rstest]
    fn submit_order_handle_round_trips_command() {
        let order = market_order("O-1");
        let order_id = order.client_order_id();
        let handle = SubmitOrderHandle::new(SubmitOrderCommand::new(order, None, None, None));
        assert_eq!(handle.command().order.client_order_id(), order_id);
        // Exercise the Deref impl explicitly so a regression in the Deref
        // target binding (e.g. returning a default) fails the test.
        assert_eq!(Deref::deref(&handle).order.client_order_id(), order_id,);
        let recovered = handle.into_inner();
        assert_eq!(recovered.order.client_order_id(), order_id);
    }

    #[rstest]
    fn submit_order_list_handle_round_trips_command() {
        let handle = SubmitOrderListHandle::new(SubmitOrderListCommand::new(
            vec![market_order("O-1"), market_order("O-2")],
            None,
            None,
            None,
        ));
        assert_eq!(handle.command().orders.len(), 2);
        // Exercise the Deref impl explicitly.
        assert_eq!(Deref::deref(&handle).orders.len(), 2);
        let recovered = handle.into_inner();
        assert_eq!(recovered.orders.len(), 2);
        assert_eq!(
            recovered.orders[0].client_order_id(),
            ClientOrderId::from("O-1")
        );
    }
}
