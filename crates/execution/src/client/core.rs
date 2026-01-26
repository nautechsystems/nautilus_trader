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

//! Base execution client functionality.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::Currency,
};

/// Base implementation for execution clients providing identity and connection state.
///
/// This struct provides the foundation for all execution clients, holding
/// client identity, connection state, and read-only cache access. Execution
/// clients use this as a base and extend it with venue-specific implementations.
///
/// For event generation, use [`OrderEventFactory`] from `nautilus_common::factories`.
/// For live adapters, use [`ExecutionEventEmitter`] which combines event generation
/// with async dispatch. For backtest/sandbox, use [`OrderEventFactory`] directly
/// and dispatch via `msgbus::send_order_event()`.
#[derive(Debug, Clone)]
pub struct ExecutionClientCore {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub is_connected: bool,
    cache: Rc<RefCell<Cache>>,
}

impl ExecutionClientCore {
    /// Creates a new [`ExecutionClientCore`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Venue,
        oms_type: OmsType,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            oms_type,
            account_id,
            account_type,
            base_currency,
            is_connected: false,
            cache,
        }
    }

    /// Returns a read-only borrow of the cache.
    pub fn cache(&self) -> std::cell::Ref<'_, Cache> {
        self.cache.borrow()
    }

    /// Sets the connection status of the execution client.
    pub const fn set_connected(&mut self, is_connected: bool) {
        self.is_connected = is_connected;
    }

    /// Sets the account identifier for the execution client.
    pub const fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }
}
