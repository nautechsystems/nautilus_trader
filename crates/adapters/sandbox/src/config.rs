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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.sandbox")
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
            trade_execution: false,
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
            self.trade_execution,
            false, // liquidity_consumption
            self.reject_stop_orders,
            self.support_gtd_orders,
            self.support_contingent_orders,
            self.use_position_ids,
            self.use_random_ids,
            self.use_reduce_only,
            false, // use_market_order_acks
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
            trade_execution: false,
            reject_stop_orders: true,
            support_gtd_orders: true,
            support_contingent_orders: true,
            use_position_ids: true,
            use_random_ids: false,
            use_reduce_only: true,
        }
    }
}

#[cfg(feature = "python")]
mod pyo3_impl {
    use nautilus_model::{
        enums::{AccountType, BookType, OmsType},
        identifiers::{AccountId, TraderId, Venue},
        types::{Currency, Money},
    };
    use pyo3::prelude::*;
    use rust_decimal::Decimal;

    use super::SandboxExecutionClientConfig;

    #[pymethods]
    impl SandboxExecutionClientConfig {
        #[new]
        #[pyo3(signature = (venue, starting_balances, trader_id=None, account_id=None, base_currency=None, oms_type=None, account_type=None, default_leverage=None, book_type=None, frozen_account=false, bar_execution=true, trade_execution=false, reject_stop_orders=true, support_gtd_orders=true, support_contingent_orders=true, use_position_ids=true, use_random_ids=false, use_reduce_only=true))]
        #[allow(clippy::too_many_arguments)]
        fn py_new(
            venue: Venue,
            starting_balances: Vec<Money>,
            trader_id: Option<TraderId>,
            account_id: Option<AccountId>,
            base_currency: Option<Currency>,
            oms_type: Option<OmsType>,
            account_type: Option<AccountType>,
            default_leverage: Option<Decimal>,
            book_type: Option<BookType>,
            frozen_account: bool,
            bar_execution: bool,
            trade_execution: bool,
            reject_stop_orders: bool,
            support_gtd_orders: bool,
            support_contingent_orders: bool,
            use_position_ids: bool,
            use_random_ids: bool,
            use_reduce_only: bool,
        ) -> Self {
            // Generate default IDs from venue if not provided
            let trader_id =
                trader_id.unwrap_or_else(|| TraderId::from(format!("{venue}-001").as_str()));
            let account_id = account_id
                .unwrap_or_else(|| AccountId::from(format!("{venue}-SANDBOX-001").as_str()));

            Self {
                trader_id,
                account_id,
                venue,
                starting_balances,
                base_currency,
                oms_type: oms_type.unwrap_or(OmsType::Netting),
                account_type: account_type.unwrap_or(AccountType::Margin),
                default_leverage: default_leverage.unwrap_or(Decimal::ONE),
                leverages: ahash::AHashMap::new(),
                book_type: book_type.unwrap_or(BookType::L1_MBP),
                frozen_account,
                bar_execution,
                trade_execution,
                reject_stop_orders,
                support_gtd_orders,
                support_contingent_orders,
                use_position_ids,
                use_random_ids,
                use_reduce_only,
            }
        }

        #[getter]
        fn trader_id(&self) -> TraderId {
            self.trader_id
        }

        #[getter]
        fn account_id(&self) -> AccountId {
            self.account_id
        }

        #[getter]
        fn venue(&self) -> Venue {
            self.venue
        }

        #[getter]
        fn starting_balances(&self) -> Vec<Money> {
            self.starting_balances.clone()
        }

        #[getter]
        fn base_currency(&self) -> Option<Currency> {
            self.base_currency
        }

        #[getter]
        fn oms_type(&self) -> OmsType {
            self.oms_type
        }

        #[getter]
        fn account_type(&self) -> AccountType {
            self.account_type
        }

        #[getter]
        fn default_leverage(&self) -> Decimal {
            self.default_leverage
        }

        #[getter]
        fn book_type(&self) -> BookType {
            self.book_type
        }

        #[getter]
        fn frozen_account(&self) -> bool {
            self.frozen_account
        }

        #[getter]
        fn bar_execution(&self) -> bool {
            self.bar_execution
        }

        #[getter]
        fn trade_execution(&self) -> bool {
            self.trade_execution
        }

        #[getter]
        fn reject_stop_orders(&self) -> bool {
            self.reject_stop_orders
        }

        #[getter]
        fn support_gtd_orders(&self) -> bool {
            self.support_gtd_orders
        }

        #[getter]
        fn support_contingent_orders(&self) -> bool {
            self.support_contingent_orders
        }

        #[getter]
        fn use_position_ids(&self) -> bool {
            self.use_position_ids
        }

        #[getter]
        fn use_random_ids(&self) -> bool {
            self.use_random_ids
        }

        #[getter]
        fn use_reduce_only(&self) -> bool {
            self.use_reduce_only
        }
    }
}
