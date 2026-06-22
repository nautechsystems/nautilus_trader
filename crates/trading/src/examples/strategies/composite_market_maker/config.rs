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

//! Configuration for the composite market making strategy.

use nautilus_model::{
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};

use crate::strategy::StrategyConfig;

/// Configuration for the composite market making strategy.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct CompositeMarketMakerConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig::builder()
        .strategy_id(StrategyId::from("COMPOSITE_MM-001"))
        .order_id_tag("001".to_string())
        .build())]
    pub base: StrategyConfig,
    /// Target instrument the strategy quotes on.
    pub instrument_id: InstrumentId,
    /// Signal instrument that drives the signal skew. Typically a
    /// `SyntheticInstrument`, but any instrument that publishes quotes works.
    pub signal_instrument_id: InstrumentId,
    /// Trade size per quote. When `None`, resolved from the instrument's
    /// `min_quantity` during `on_start`.
    pub trade_size: Option<Quantity>,
    /// Half of the desired quoted spread, in basis points of the anchor.
    /// E.g. `5` = 5 bps, so the full quoted spread is 10 bps before skew.
    #[builder(default = 5)]
    pub half_spread_bps: u32,
    /// Inventory skew gain in price units per unit of net position. Both sides
    /// shift down by `factor * net_position` so a long position widens the bid
    /// and tightens the ask.
    #[builder(default = 0.0)]
    pub inventory_skew_factor: f64,
    /// Signal skew gain in price units per unit of normalized signal residual.
    /// Both sides shift up by `factor * residual` where
    /// `residual = (signal_mid - baseline) / baseline`.
    #[builder(default = 0.0)]
    pub signal_skew_factor: f64,
    /// Optional baseline price for the signal residual. When `None`, the first
    /// observed signal mid is captured as the baseline. When `Some(_)`, the
    /// configured value is used so backtests are deterministic.
    pub signal_baseline: Option<f64>,
    /// Hard cap on net exposure (long or short).
    pub max_position: Quantity,
    /// Minimum movement in basis points of the anchor before re-quoting.
    /// Applied to both anchor movement and the signal residual's price impact
    /// (`signal_skew_factor * residual`); whichever clears the threshold first
    /// triggers a requote.
    #[builder(default = 5)]
    pub requote_threshold_bps: u32,
    /// Optional order expiry in seconds. When set, orders use GTD time-in-force
    /// with `expire_time = now + expire_time_secs`.
    pub expire_time_secs: Option<u64>,
    /// When `true`, resubmit on the next quote tick after an external cancel.
    /// Useful for venues that proactively cancel short-term orders.
    #[builder(default = false)]
    pub on_cancel_resubmit: bool,
}
