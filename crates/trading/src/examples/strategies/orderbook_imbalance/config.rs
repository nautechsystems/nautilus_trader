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

//! Configuration for the order book imbalance strategy.

use nautilus_core::datetime::secs_to_nanos;
use nautilus_model::{
    enums::BookType,
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};
use rust_decimal::Decimal;

use crate::strategy::StrategyConfig;

/// Configuration for the current-book imbalance strategy.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct OrderBookImbalanceConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("OrderBookImbalance-000")),
        order_id_tag: Some("000".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    /// Instrument to subscribe to and trade.
    pub instrument_id: InstrumentId,
    /// Maximum order size, truncated to the instrument's size precision.
    pub max_trade_size: Quantity,
    /// The larger best size must exceed this threshold.
    #[builder(default = Decimal::from(100))]
    pub trigger_min_size: Decimal,
    /// The smaller/larger best-size ratio must be below this threshold in (0, 1).
    #[builder(default = Decimal::new(20, 2))]
    pub trigger_imbalance_ratio: Decimal,
    /// Minimum elapsed seconds between eligible signals.
    #[builder(default = 1.0)]
    pub min_seconds_between_triggers: f64,
    /// Book representation used by the managed subscription.
    #[builder(default = BookType::L2_MBP)]
    pub book_type: BookType,
    /// Reports signals without submitting, canceling, or closing orders, including on stop.
    #[builder(default)]
    pub dry_run: bool,
    /// Snapshot logging interval in book updates, or zero to disable.
    #[builder(default)]
    pub log_interval: u64,
}

impl OrderBookImbalanceConfig {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.max_trade_size.is_positive(),
            "max_trade_size must be positive"
        );
        anyhow::ensure!(
            self.trigger_min_size >= Decimal::ZERO,
            "trigger_min_size must be non-negative"
        );
        anyhow::ensure!(
            self.trigger_imbalance_ratio > Decimal::ZERO
                && self.trigger_imbalance_ratio < Decimal::ONE,
            "trigger_imbalance_ratio must be in (0, 1)"
        );
        anyhow::ensure!(
            self.min_seconds_between_triggers >= 0.0
                && secs_to_nanos(self.min_seconds_between_triggers).is_ok(),
            "min_seconds_between_triggers must be finite, non-negative, and representable in nanoseconds"
        );
        anyhow::ensure!(
            !self.dry_run
                || !(self.base.manage_stop
                    || self.base.manage_gtd_expiry
                    || self.base.manage_contingent_orders),
            "dry_run requires manage_stop, manage_gtd_expiry, and manage_contingent_orders=false"
        );
        Ok(())
    }
}
