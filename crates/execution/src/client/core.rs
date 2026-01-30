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

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

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
#[derive(Debug)]
pub struct ExecutionClientCore {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    connected: AtomicBool,
    started: AtomicBool,
    instruments_initialized: AtomicBool,
    cache: Rc<RefCell<Cache>>,
}

impl Clone for ExecutionClientCore {
    fn clone(&self) -> Self {
        Self {
            trader_id: self.trader_id,
            client_id: self.client_id,
            venue: self.venue,
            oms_type: self.oms_type,
            account_id: self.account_id,
            account_type: self.account_type,
            base_currency: self.base_currency,
            connected: AtomicBool::new(self.connected.load(Ordering::Acquire)),
            started: AtomicBool::new(self.started.load(Ordering::Acquire)),
            instruments_initialized: AtomicBool::new(
                self.instruments_initialized.load(Ordering::Acquire),
            ),
            cache: self.cache.clone(),
        }
    }
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
            connected: AtomicBool::new(false),
            started: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            cache,
        }
    }

    /// Returns a read-only borrow of the cache.
    pub fn cache(&self) -> std::cell::Ref<'_, Cache> {
        self.cache.borrow()
    }

    /// Returns `true` if the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Returns `true` if the client is disconnected.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    /// Sets the client as connected.
    pub fn set_connected(&self) {
        self.connected.store(true, Ordering::Release);
    }

    /// Sets the client as disconnected.
    pub fn set_disconnected(&self) {
        self.connected.store(false, Ordering::Release);
    }

    /// Returns `true` if the client has been started.
    #[must_use]
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Returns `true` if the client has not been started.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        !self.is_started()
    }

    /// Sets the client as started.
    pub fn set_started(&self) {
        self.started.store(true, Ordering::Release);
    }

    /// Sets the client as stopped.
    pub fn set_stopped(&self) {
        self.started.store(false, Ordering::Release);
    }

    /// Returns `true` if instruments have been initialized.
    #[must_use]
    pub fn instruments_initialized(&self) -> bool {
        self.instruments_initialized.load(Ordering::Acquire)
    }

    /// Sets instruments as initialized.
    pub fn set_instruments_initialized(&self) {
        self.instruments_initialized.store(true, Ordering::Release);
    }

    /// Sets the account identifier for the execution client.
    pub const fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }
}
