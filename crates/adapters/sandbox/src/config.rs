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
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.sandbox", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.sandbox")
)]
pub struct SandboxExecutionClientConfig {
    /// The trader ID for this client.
    #[builder(default = TraderId::from("SANDBOX-001"))]
    pub trader_id: TraderId,
    /// The account ID for this client.
    #[builder(default = AccountId::from("SANDBOX-001"))]
    pub account_id: AccountId,
    /// The venue for this sandbox execution client.
    #[builder(default = Venue::new("SANDBOX"))]
    pub venue: Venue,
    /// The starting balances for this sandbox venue.
    #[builder(default)]
    pub starting_balances: Vec<Money>,
    /// The base currency for this venue (None for multi-currency).
    pub base_currency: Option<Currency>,
    /// The order management system type used by the exchange.
    #[builder(default = OmsType::Netting)]
    pub oms_type: OmsType,
    /// The account type for the client.
    #[builder(default = AccountType::Margin)]
    pub account_type: AccountType,
    /// The account default leverage (for margin accounts).
    #[builder(default = Decimal::ONE)]
    pub default_leverage: Decimal,
    /// Per-instrument leverage overrides.
    #[builder(default)]
    pub leverages: AHashMap<InstrumentId, Decimal>,
    /// The order book type for the matching engine.
    #[builder(default = BookType::L1_MBP)]
    pub book_type: BookType,
    /// If True, account balances won't change (frozen).
    #[builder(default)]
    pub frozen_account: bool,
    /// If bars should be processed by the matching engine (and move the market).
    #[builder(default = true)]
    pub bar_execution: bool,
    /// If trades should be processed by the matching engine (and move the market).
    #[builder(default = true)]
    pub trade_execution: bool,
    /// If stop orders are rejected on submission if trigger price is in the market.
    #[builder(default = true)]
    pub reject_stop_orders: bool,
    /// If orders with GTD time in force will be supported by the venue.
    #[builder(default = true)]
    pub support_gtd_orders: bool,
    /// If contingent orders will be supported/respected by the venue.
    #[builder(default = true)]
    pub support_contingent_orders: bool,
    /// If venue position IDs will be generated on order fills.
    #[builder(default = true)]
    pub use_position_ids: bool,
    /// If venue order IDs and position IDs will be random UUID4's.
    /// Trade IDs are always deterministic and not affected by this flag.
    #[builder(default)]
    pub use_random_ids: bool,
    /// If the `reduce_only` execution instruction on orders will be honored.
    #[builder(default = true)]
    pub use_reduce_only: bool,
}

impl SandboxExecutionClientConfig {
    /// Creates an [`OrderMatchingEngineConfig`] from this sandbox config.
    #[must_use]
    pub fn to_matching_engine_config(&self) -> OrderMatchingEngineConfig {
        OrderMatchingEngineConfig::builder()
            .bar_execution(self.bar_execution)
            .trade_execution(self.trade_execution)
            .reject_stop_orders(self.reject_stop_orders)
            .support_gtd_orders(self.support_gtd_orders)
            .support_contingent_orders(self.support_contingent_orders)
            .use_position_ids(self.use_position_ids)
            .use_random_ids(self.use_random_ids)
            .use_reduce_only(self.use_reduce_only)
            .build()
    }
}

impl Default for SandboxExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}
