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

//! Configuration for the EMA crossover strategy.

use nautilus_model::{
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};

use crate::strategy::StrategyConfig;

/// Configuration for the dual-EMA crossover strategy.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
pub struct EmaCrossConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Instrument to subscribe to and trade.
    pub instrument_id: InstrumentId,
    /// Order quantity for each crossover signal.
    pub trade_size: Quantity,
    /// Fast EMA period. Shorter periods react faster.
    pub fast_period: usize,
    /// Slow EMA period. Longer periods filter noise.
    pub slow_period: usize,
}

impl EmaCrossConfig {
    /// Creates a new [`EmaCrossConfig`] with required fields and sensible defaults.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        fast_period: usize,
        slow_period: usize,
    ) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(StrategyId::from("EMA_CROSS-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            },
            instrument_id,
            trade_size,
            fast_period,
            slow_period,
        }
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
impl EmaCrossConfig {
    #[new]
    #[pyo3(signature = (
        instrument_id,
        trade_size,
        fast_period=10,
        slow_period=50,
        strategy_id=None,
        order_id_tag=None,
    ))]
    fn py_new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        fast_period: usize,
        slow_period: usize,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
    ) -> Self {
        let mut config = Self::new(instrument_id, trade_size, fast_period, slow_period);

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
    fn trade_size(&self) -> Quantity {
        self.trade_size
    }

    #[getter]
    fn fast_period(&self) -> usize {
        self.fast_period
    }

    #[getter]
    fn slow_period(&self) -> usize {
        self.slow_period
    }
}
