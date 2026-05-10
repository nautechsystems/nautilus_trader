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

//! Configuration for the grid market making strategy.

use nautilus_model::{
    identifiers::{InstrumentId, StrategyId},
    types::Quantity,
};

use crate::strategy::StrategyConfig;

/// Configuration for the grid market making strategy.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
pub struct GridMarketMakerConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Instrument ID to trade.
    pub instrument_id: InstrumentId,
    /// Trade size per grid level. When `None` the strategy resolves it from
    /// the instrument's `min_quantity` during `on_start`.
    pub trade_size: Option<Quantity>,
    /// Number of price levels on each side (buy & sell).
    pub num_levels: usize,
    /// Grid spacing in basis points of mid-price (geometric grid).
    /// E.g. `10` = 10 bps = 0.1%. Buy level N = mid × (1 - bps/10000)^N.
    pub grid_step_bps: u32,
    /// How aggressively to shift the grid based on inventory.
    pub skew_factor: f64,
    /// Hard cap on net exposure (long or short).
    pub max_position: Quantity,
    /// Minimum mid-price move in basis points before re-quoting.
    /// E.g. `5` = 5 bps = 0.05%.
    pub requote_threshold_bps: u32,
    /// Optional order expiry in seconds. When set, orders use GTD
    /// time-in-force with `expire_time = now + expire_time_secs`.
    pub expire_time_secs: Option<u64>,
    /// When `true`, resubmit the full grid on the next quote after receiving
    /// an order cancel event. Useful for exchanges like dYdX where short-term
    /// orders are canceled by the protocol after expiry.
    pub on_cancel_resubmit: bool,
}

impl GridMarketMakerConfig {
    /// Creates a new [`GridMarketMakerConfig`] with required fields and sensible defaults.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, max_position: Quantity) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(StrategyId::from("GRID_MM-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            },
            instrument_id,
            trade_size: None,
            num_levels: 3,
            grid_step_bps: 10,
            skew_factor: 0.0,
            max_position,
            requote_threshold_bps: 5,
            expire_time_secs: None,
            on_cancel_resubmit: false,
        }
    }

    #[must_use]
    pub fn with_trade_size(mut self, trade_size: Quantity) -> Self {
        self.trade_size = Some(trade_size);
        self
    }

    #[must_use]
    pub fn with_num_levels(mut self, num_levels: usize) -> Self {
        self.num_levels = num_levels;
        self
    }

    #[must_use]
    pub fn with_grid_step_bps(mut self, bps: u32) -> Self {
        self.grid_step_bps = bps;
        self
    }

    #[must_use]
    pub fn with_skew_factor(mut self, skew_factor: f64) -> Self {
        self.skew_factor = skew_factor;
        self
    }

    #[must_use]
    pub fn with_requote_threshold_bps(mut self, bps: u32) -> Self {
        self.requote_threshold_bps = bps;
        self
    }

    #[must_use]
    pub fn with_expire_time_secs(mut self, secs: u64) -> Self {
        self.expire_time_secs = Some(secs);
        self
    }

    #[must_use]
    pub fn with_on_cancel_resubmit(mut self, enabled: bool) -> Self {
        self.on_cancel_resubmit = enabled;
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
impl GridMarketMakerConfig {
    #[new]
    #[pyo3(signature = (
        instrument_id,
        max_position,
        strategy_id=None,
        order_id_tag=None,
        trade_size=None,
        num_levels=3,
        grid_step_bps=10,
        skew_factor=0.0,
        requote_threshold_bps=5,
        expire_time_secs=None,
        on_cancel_resubmit=false,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        max_position: Quantity,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        trade_size: Option<Quantity>,
        num_levels: usize,
        grid_step_bps: u32,
        skew_factor: f64,
        requote_threshold_bps: u32,
        expire_time_secs: Option<u64>,
        on_cancel_resubmit: bool,
    ) -> Self {
        let mut config = Self::new(instrument_id, max_position)
            .with_num_levels(num_levels)
            .with_grid_step_bps(grid_step_bps)
            .with_skew_factor(skew_factor)
            .with_requote_threshold_bps(requote_threshold_bps)
            .with_on_cancel_resubmit(on_cancel_resubmit);

        if let Some(size) = trade_size {
            config = config.with_trade_size(size);
        }

        if let Some(secs) = expire_time_secs {
            config = config.with_expire_time_secs(secs);
        }

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
    fn max_position(&self) -> Quantity {
        self.max_position
    }

    #[getter]
    fn trade_size(&self) -> Option<Quantity> {
        self.trade_size
    }

    #[getter]
    fn num_levels(&self) -> usize {
        self.num_levels
    }

    #[getter]
    fn grid_step_bps(&self) -> u32 {
        self.grid_step_bps
    }

    #[getter]
    fn skew_factor(&self) -> f64 {
        self.skew_factor
    }

    #[getter]
    fn requote_threshold_bps(&self) -> u32 {
        self.requote_threshold_bps
    }

    #[getter]
    fn expire_time_secs(&self) -> Option<u64> {
        self.expire_time_secs
    }

    #[getter]
    fn on_cancel_resubmit(&self) -> bool {
        self.on_cancel_resubmit
    }
}
