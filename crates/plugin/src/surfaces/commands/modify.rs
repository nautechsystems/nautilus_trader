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

//! Modify-order command and its boundary-owned handle.
//!
//! The plug-in constructs a [`ModifyOrderCommand`], wraps it in a
//! [`ModifyOrderHandle`], and hands the host a `*const ModifyOrderHandle`
//! via [`HostVTable::modify_order`](crate::host::HostVTable::modify_order).
//! The host derefs the handle once and routes the borrowed command into
//! the calling strategy's modify path. The plug-in owns the box and frees
//! it when the call returns.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_core::Params;
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId},
    types::{Price, Quantity},
};

/// Modify-order command. Mirrors the arguments to `Strategy::modify_order`.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyOrderCommand {
    /// The client order identifier of the order to modify.
    pub client_order_id: ClientOrderId,

    /// New order quantity.
    pub quantity: Option<Quantity>,

    /// New limit price.
    pub price: Option<Price>,

    /// New trigger price.
    pub trigger_price: Option<Price>,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl ModifyOrderCommand {
    /// Creates a new [`ModifyOrderCommand`] instance.
    #[must_use]
    pub const fn new(
        client_order_id: ClientOrderId,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_order_id,
            quantity,
            price,
            trigger_price,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`ModifyOrderCommand`] cross the cdylib
/// FFI boundary by reference.
///
/// The plug-in constructs an instance, hands a
/// `*const ModifyOrderHandle` to the host for the duration of the
/// `modify_order` call, and drops the handle when the call returns. The
/// host only borrows the handle and never owns it.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ModifyOrderHandle(Box<ModifyOrderCommand>);

impl ModifyOrderHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: ModifyOrderCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &ModifyOrderCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> ModifyOrderCommand {
        *self.0
    }
}

impl Deref for ModifyOrderHandle {
    type Target = ModifyOrderCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::ClientOrderId;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn modify_order_handle_round_trips_command() {
        let cmd = ModifyOrderCommand::new(
            ClientOrderId::from("O-1"),
            Some(Quantity::from("2.5")),
            Some(Price::from("100.00")),
            None,
            None,
            None,
        );
        let handle = ModifyOrderHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }
}
