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
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
pub struct HurstVpinDirectionalConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Instrument to subscribe to and trade.
    pub instrument_id: InstrumentId,
    /// Dollar bar type (value aggregation sourced from trades).
    pub bar_type: BarType,
    /// Order quantity for each entry.
    pub trade_size: Quantity,
    /// Rolling window of dollar bar returns used to estimate the Hurst exponent.
    pub hurst_window: usize,
    /// Lag set used for rescaled range regression.
    pub hurst_lags: Vec<usize>,
    /// Hurst threshold for entering a position (trending regime).
    pub hurst_enter: f64,
    /// Hurst threshold for exiting an open position (regime decay).
    pub hurst_exit: f64,
    /// Number of completed volume buckets averaged for VPIN.
    pub vpin_window: usize,
    /// Minimum VPIN value required to treat a bucket imbalance as informed flow.
    pub vpin_threshold: f64,
    /// Maximum time (seconds) a position is held before forced flatten.
    pub max_holding_secs: u64,
}

impl HurstVpinDirectionalConfig {
    /// Creates a new [`HurstVpinDirectionalConfig`] with required fields and sensible defaults.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, bar_type: BarType, trade_size: Quantity) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(StrategyId::from("HURST_VPIN-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            },
            instrument_id,
            bar_type,
            trade_size,
            hurst_window: 128,
            hurst_lags: vec![4, 8, 16, 32],
            hurst_enter: 0.55,
            hurst_exit: 0.50,
            vpin_window: 50,
            vpin_threshold: 0.30,
            max_holding_secs: 3600,
        }
    }

    #[must_use]
    pub fn with_hurst_window(mut self, window: usize) -> Self {
        self.hurst_window = window;
        self
    }

    #[must_use]
    pub fn with_hurst_lags(mut self, lags: Vec<usize>) -> Self {
        self.hurst_lags = lags;
        self
    }

    #[must_use]
    pub fn with_hurst_enter(mut self, threshold: f64) -> Self {
        self.hurst_enter = threshold;
        self
    }

    #[must_use]
    pub fn with_hurst_exit(mut self, threshold: f64) -> Self {
        self.hurst_exit = threshold;
        self
    }

    #[must_use]
    pub fn with_vpin_window(mut self, window: usize) -> Self {
        self.vpin_window = window;
        self
    }

    #[must_use]
    pub fn with_vpin_threshold(mut self, threshold: f64) -> Self {
        self.vpin_threshold = threshold;
        self
    }

    #[must_use]
    pub fn with_max_holding_secs(mut self, secs: u64) -> Self {
        self.max_holding_secs = secs;
        self
    }

    #[must_use]
    pub fn with_strategy_id(mut self, strategy_id: StrategyId) -> Self {
        self.base.strategy_id = Some(strategy_id);
        self
    }

    #[must_use]
    pub fn with_order_id_tag(mut self, tag: String) -> Self {
        self.base.order_id_tag = Some(tag);
        self
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl HurstVpinDirectionalConfig {
    #[new]
    #[pyo3(signature = (
        instrument_id,
        bar_type,
        trade_size,
        strategy_id=None,
        order_id_tag=None,
        hurst_window=128,
        hurst_lags=vec![4, 8, 16, 32],
        hurst_enter=0.55,
        hurst_exit=0.50,
        vpin_window=50,
        vpin_threshold=0.30,
        max_holding_secs=3600,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Quantity,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        hurst_window: usize,
        hurst_lags: Vec<usize>,
        hurst_enter: f64,
        hurst_exit: f64,
        vpin_window: usize,
        vpin_threshold: f64,
        max_holding_secs: u64,
    ) -> Self {
        let mut config = Self::new(instrument_id, bar_type, trade_size)
            .with_hurst_window(hurst_window)
            .with_hurst_lags(hurst_lags)
            .with_hurst_enter(hurst_enter)
            .with_hurst_exit(hurst_exit)
            .with_vpin_window(vpin_window)
            .with_vpin_threshold(vpin_threshold)
            .with_max_holding_secs(max_holding_secs);

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn bar_type(&self) -> BarType {
        self.bar_type
    }

    #[getter]
    fn trade_size(&self) -> Quantity {
        self.trade_size
    }

    #[getter]
    fn hurst_window(&self) -> usize {
        self.hurst_window
    }

    #[getter]
    fn hurst_lags(&self) -> Vec<usize> {
        self.hurst_lags.clone()
    }

    #[getter]
    fn hurst_enter(&self) -> f64 {
        self.hurst_enter
    }

    #[getter]
    fn hurst_exit(&self) -> f64 {
        self.hurst_exit
    }

    #[getter]
    fn vpin_window(&self) -> usize {
        self.vpin_window
    }

    #[getter]
    fn vpin_threshold(&self) -> f64 {
        self.vpin_threshold
    }

    #[getter]
    fn max_holding_secs(&self) -> u64 {
        self.max_holding_secs
    }
}
