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

//! Configuration for sandbox execution client.

use ahash::AHashMap;
use nautilus_execution::matching_engine::config::OrderMatchingEngineConfig;
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, InstrumentId, TraderId, Venue},
    types::{Currency, Money},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for `SandboxExecutionClient` instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.sandbox", from_py_object)
)]
pub struct SandboxExecutionClientConfig {
    /// The trader ID for this client.
    pub trader_id: TraderId,
    /// The account ID for this client.
    pub account_id: AccountId,
    /// The venue for this sandbox execution client.
    pub venue: Venue,
    /// The starting balances for this sandbox venue.
    pub starting_balances: Vec<Money>,
    /// The base currency for this venue (None for multi-currency).
    pub base_currency: Option<Currency>,
    /// The order management system type used by the exchange.
    pub oms_type: OmsType,
    /// The account type for the client.
    pub account_type: AccountType,
    /// The account default leverage (for margin accounts).
    pub default_leverage: Decimal,
    /// Per-instrument leverage overrides.
    pub leverages: AHashMap<InstrumentId, Decimal>,
    /// The order book type for the matching engine.
    pub book_type: BookType,
    /// If True, account balances won't change (frozen).
    pub frozen_account: bool,
    /// If bars should be processed by the matching engine (and move the market).
    pub bar_execution: bool,
    /// If trades should be processed by the matching engine (and move the market).
    pub trade_execution: bool,
    /// If stop orders are rejected on submission if trigger price is in the market.
    pub reject_stop_orders: bool,
    /// If orders with GTD time in force will be supported by the venue.
    pub support_gtd_orders: bool,
    /// If contingent orders will be supported/respected by the venue.
    pub support_contingent_orders: bool,
    /// If venue position IDs will be generated on order fills.
    pub use_position_ids: bool,
    /// If all venue generated identifiers will be random UUID4's.
    pub use_random_ids: bool,
    /// If the `reduce_only` execution instruction on orders will be honored.
    pub use_reduce_only: bool,
}

impl SandboxExecutionClientConfig {
    /// Creates a new [`SandboxExecutionClientConfig`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        account_id: AccountId,
        venue: Venue,
        starting_balances: Vec<Money>,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            venue,
            starting_balances,
            base_currency: None,
            oms_type: OmsType::Netting,
            account_type: AccountType::Margin,
            default_leverage: Decimal::ONE,
            leverages: AHashMap::new(),
            book_type: BookType::L1_MBP,
            frozen_account: false,
            bar_execution: true,
            trade_execution: true,
            reject_stop_orders: true,
            support_gtd_orders: true,
            support_contingent_orders: true,
            use_position_ids: true,
            use_random_ids: false,
            use_reduce_only: true,
        }
    }

    /// Creates an [`OrderMatchingEngineConfig`] from this sandbox config.
    #[must_use]
    pub fn to_matching_engine_config(&self) -> OrderMatchingEngineConfig {
        OrderMatchingEngineConfig::new(
            self.bar_execution,
            false, // bar_adaptive_high_low_ordering
            self.trade_execution,
            false, // liquidity_consumption
            self.reject_stop_orders,
            self.support_gtd_orders,
            self.support_contingent_orders,
            self.use_position_ids,
            self.use_random_ids,
            self.use_reduce_only,
            false, // use_market_order_acks
            false, // queue_position
            false, // oto_full_trigger
        )
    }

    /// Sets the base currency.
    #[must_use]
    pub fn with_base_currency(mut self, currency: Currency) -> Self {
        self.base_currency = Some(currency);
        self
    }

    /// Sets the OMS type.
    #[must_use]
    pub fn with_oms_type(mut self, oms_type: OmsType) -> Self {
        self.oms_type = oms_type;
        self
    }

    /// Sets the account type.
    #[must_use]
    pub fn with_account_type(mut self, account_type: AccountType) -> Self {
        self.account_type = account_type;
        self
    }

    /// Sets the default leverage.
    #[must_use]
    pub fn with_default_leverage(mut self, leverage: Decimal) -> Self {
        self.default_leverage = leverage;
        self
    }

    /// Sets the book type.
    #[must_use]
    pub fn with_book_type(mut self, book_type: BookType) -> Self {
        self.book_type = book_type;
        self
    }

    /// Sets whether the account is frozen.
    #[must_use]
    pub fn with_frozen_account(mut self, frozen: bool) -> Self {
        self.frozen_account = frozen;
        self
    }

    /// Sets whether bar execution is enabled.
    #[must_use]
    pub fn with_bar_execution(mut self, enabled: bool) -> Self {
        self.bar_execution = enabled;
        self
    }

    /// Sets whether trade execution is enabled.
    #[must_use]
    pub fn with_trade_execution(mut self, enabled: bool) -> Self {
        self.trade_execution = enabled;
        self
    }
}

impl Default for SandboxExecutionClientConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::from("SANDBOX-001"),
            account_id: AccountId::from("SANDBOX-001"),
            venue: Venue::new("SANDBOX"),
            starting_balances: Vec::new(),
            base_currency: None,
            oms_type: OmsType::Netting,
            account_type: AccountType::Margin,
            default_leverage: Decimal::ONE,
            leverages: AHashMap::new(),
            book_type: BookType::L1_MBP,
            frozen_account: false,
            bar_execution: true,
            trade_execution: true,
            reject_stop_orders: true,
            support_gtd_orders: true,
            support_contingent_orders: true,
            use_position_ids: true,
            use_random_ids: false,
            use_reduce_only: true,
        }
    }
}
