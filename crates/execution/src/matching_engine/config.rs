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

use serde::{Deserialize, Serialize};

/// Configuration for `OrderMatchingEngine` instances.
#[derive(Debug, Clone, Deserialize, Serialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct OrderMatchingEngineConfig {
    #[builder(default = true)]
    pub bar_execution: bool,
    #[builder(default)]
    pub bar_adaptive_high_low_ordering: bool,
    #[builder(default = true)]
    pub trade_execution: bool,
    #[builder(default)]
    pub liquidity_consumption: bool,
    #[builder(default = true)]
    pub reject_stop_orders: bool,
    #[builder(default = true)]
    pub support_gtd_orders: bool,
    #[builder(default = true)]
    pub support_contingent_orders: bool,
    #[builder(default = true)]
    pub use_position_ids: bool,
    #[builder(default)]
    pub use_random_ids: bool,
    #[builder(default = true)]
    pub use_reduce_only: bool,
    #[builder(default)]
    pub use_market_order_acks: bool,
    #[builder(default)]
    pub queue_position: bool,
    #[builder(default)]
    pub oto_full_trigger: bool,
    pub price_protection_points: Option<u32>,
}

impl Default for OrderMatchingEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Locks Rust defaults to the Cython per-engine constructor at
    // nautilus_trader/backtest/engine.pyx:3882-3894.
    #[rstest]
    fn test_default_matches_cython() {
        let config = OrderMatchingEngineConfig::default();
        assert!(config.bar_execution);
        assert!(!config.bar_adaptive_high_low_ordering);
        assert!(config.trade_execution);
        assert!(!config.liquidity_consumption);
        assert!(config.reject_stop_orders);
        assert!(config.support_gtd_orders);
        assert!(config.support_contingent_orders);
        assert!(config.use_position_ids);
        assert!(!config.use_random_ids);
        assert!(config.use_reduce_only);
        assert!(!config.use_market_order_acks);
        assert!(!config.queue_position);
        assert!(!config.oto_full_trigger);
        assert_eq!(config.price_protection_points, None);
    }
}
