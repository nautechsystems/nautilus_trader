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

//! Query commands and their boundary-owned handles.
//!
//! The plug-in constructs a [`QueryAccountCommand`] or [`QueryOrderCommand`],
//! wraps it in the matching `*Handle`, and hands the host a pointer via
//! [`HostVTable::query_account`](crate::host::HostVTable::query_account) or
//! [`HostVTable::query_order`](crate::host::HostVTable::query_order). The
//! host derefs the handle once and routes the borrowed command into the
//! calling strategy's query path. The plug-in owns the box and frees it
//! when the call returns.

#![allow(unsafe_code)]

use std::ops::Deref;

use nautilus_core::Params;
use nautilus_model::identifiers::{AccountId, ClientId, ClientOrderId};

/// Query-account command. Mirrors the arguments to `Strategy::query_account`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryAccountCommand {
    /// The account identifier to query.
    pub account_id: AccountId,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl QueryAccountCommand {
    /// Creates a new [`QueryAccountCommand`] instance.
    #[must_use]
    pub const fn new(
        account_id: AccountId,
        client_id: Option<ClientId>,
        params: Option<Params>,
    ) -> Self {
        Self {
            account_id,
            client_id,
            params,
        }
    }
}

/// Boundary-owned wrapper that lets [`QueryAccountCommand`] cross the cdylib
/// FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct QueryAccountHandle(Box<QueryAccountCommand>);

impl QueryAccountHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: QueryAccountCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &QueryAccountCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> QueryAccountCommand {
        *self.0
    }
}

impl Deref for QueryAccountHandle {
    type Target = QueryAccountCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Query-order command. Mirrors the arguments to `Strategy::query_order`.
///
/// The host resolves `client_order_id` against the live cache to materialise
/// the `&OrderAny` reference the trait method requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryOrderCommand {
    /// The client order identifier of the order to query.
    pub client_order_id: ClientOrderId,

    /// Optional client routing identifier.
    pub client_id: Option<ClientId>,

    /// Optional venue-specific parameters.
    pub params: Option<Params>,
}

impl QueryOrderCommand {
    /// Creates a new [`QueryOrderCommand`] instance.
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

/// Boundary-owned wrapper that lets [`QueryOrderCommand`] cross the cdylib
/// FFI boundary by reference.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct QueryOrderHandle(Box<QueryOrderCommand>);

impl QueryOrderHandle {
    /// Wraps `command` in a boundary-owned handle.
    #[must_use]
    pub fn new(command: QueryOrderCommand) -> Self {
        Self(Box::new(command))
    }

    /// Returns a reference to the wrapped command.
    #[must_use]
    pub fn command(&self) -> &QueryOrderCommand {
        &self.0
    }

    /// Consumes the wrapper and returns the inner command.
    #[must_use]
    pub fn into_inner(self) -> QueryOrderCommand {
        *self.0
    }
}

impl Deref for QueryOrderHandle {
    type Target = QueryOrderCommand;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn query_account_handle_round_trips_command() {
        let cmd = QueryAccountCommand::new(AccountId::from("BINANCE-001"), None, None);
        let handle = QueryAccountHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }

    #[rstest]
    fn query_order_handle_round_trips_command() {
        let cmd = QueryOrderCommand::new(ClientOrderId::from("O-1"), None, None);
        let handle = QueryOrderHandle::new(cmd.clone());
        assert_eq!(handle.command(), &cmd);
        assert_eq!(&*handle, &cmd);
        assert_eq!(handle.into_inner(), cmd);
    }
}
