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

//! Configuration for the complement arbitrage strategy.

use nautilus_model::identifiers::{ClientId, StrategyId, Venue};
use rust_decimal::Decimal;

use crate::strategy::StrategyConfig;

fn default_base_config() -> StrategyConfig {
    StrategyConfig {
        strategy_id: Some(StrategyId::from("COMPLEMENT_ARB-001")),
        order_id_tag: Some("001".to_string()),
        ..Default::default()
    }
}

/// Configuration for the complement arbitrage strategy.
#[derive(Debug, Clone, bon::Builder)]
pub struct ComplementArbConfig {
    /// Base strategy configuration.
    #[builder(default = default_base_config())]
    pub base: StrategyConfig,
    /// Venue to scan for binary option instruments.
    pub venue: Venue,
    /// Optional client ID for data subscriptions and order routing.
    pub client_id: Option<ClientId>,
    /// Conservative fee estimate in basis points (default: 0 = no fee).
    #[builder(default)]
    pub fee_estimate_bps: Decimal,
    /// Minimum profit in basis points after fees to trigger arb (default: 50 = 0.5%).
    #[builder(default = Decimal::new(50, 0))]
    pub min_profit_bps: Decimal,
    /// Minimum absolute profit in dollars per arb to trigger (default: 0.0 = disabled).
    #[builder(default)]
    pub min_profit_abs: Decimal,
    /// Number of shares per leg (default: 10).
    #[builder(default = Decimal::new(10, 0))]
    pub trade_size: Decimal,
    /// Maximum simultaneous in-flight arb executions across all pairs (default: 1).
    ///
    /// Note: per-market concurrency is already capped at 1 because each pair is keyed
    /// by `condition_id` and `has_active_arb` rejects a second arb on the same pair.
    /// This setting bounds total concurrent arbs across the strategy.
    #[builder(default = 1)]
    pub max_concurrent_arbs: usize,
    /// Use post-only orders for maker fee = 0% (default: true).
    #[builder(default = true)]
    pub use_post_only: bool,
    /// Order expiry in seconds for GTD time-in-force (default: 15).
    #[builder(default = 15)]
    pub order_expire_secs: u64,
    /// Slippage tolerance in bps for IOC unwind orders (default: 50).
    #[builder(default = Decimal::new(50, 0))]
    pub unwind_slippage_bps: Decimal,
    /// Master switch: submit orders when true, detect-only when false (default: false).
    #[builder(default = false)]
    pub live_trading: bool,
}
