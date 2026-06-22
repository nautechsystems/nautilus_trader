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

//! Configuration for the Hurst/VPIN directional strategy.

use nautilus_model::{
    data::BarType,
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};

use crate::strategy::StrategyConfig;

/// Configuration for the Hurst/VPIN directional strategy.
///
/// Combines a rescaled-range Hurst regime filter on dollar bars with a
/// VPIN-derived informed-flow signal, and gates entry timing on the
/// live quote stream.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct HurstVpinDirectionalConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig::builder()
        .strategy_id(StrategyId::from("HURST_VPIN-001"))
        .order_id_tag("001".to_string())
        .build())]
    pub base: StrategyConfig,
    /// Instrument to subscribe to and trade.
    pub instrument_id: InstrumentId,
    /// Dollar bar type (value aggregation sourced from trades).
    pub bar_type: BarType,
    /// Order quantity for each entry.
    pub trade_size: Quantity,
    /// Rolling window of dollar bar returns used to estimate the Hurst exponent.
    #[builder(default = 128)]
    pub hurst_window: usize,
    /// Lag set used for rescaled range regression.
    #[builder(default = vec![4, 8, 16, 32])]
    pub hurst_lags: Vec<usize>,
    /// Hurst threshold for entering a position (trending regime).
    #[builder(default = 0.55)]
    pub hurst_enter: f64,
    /// Hurst threshold for exiting an open position (regime decay).
    #[builder(default = 0.50)]
    pub hurst_exit: f64,
    /// Number of completed volume buckets averaged for VPIN.
    #[builder(default = 50)]
    pub vpin_window: usize,
    /// Minimum VPIN value required to treat a bucket imbalance as informed flow.
    #[builder(default = 0.30)]
    pub vpin_threshold: f64,
    /// Maximum time (seconds) a position is held before forced flatten.
    #[builder(default = 3600)]
    pub max_holding_secs: u64,
}
