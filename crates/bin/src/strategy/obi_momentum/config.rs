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

//! Configuration for the order book imbalance momentum strategy.

use nautilus_model::identifiers::{InstrumentId, StrategyId};
use nautilus_trading::StrategyConfig;

use crate::config::ObiMomentumTomlConfig;

/// Configuration for the order book imbalance momentum strategy.
#[derive(Debug, Clone, bon::Builder)]
pub struct ObiMomentumConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("OBI_MOM-001")),
        order_id_tag: Some("004".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    /// Instrument ID to trade.
    pub instrument_id: InstrumentId,
    /// Number of price levels on each side used for the imbalance computation.
    #[builder(default = 5)]
    pub num_levels: usize,
    /// When `true`, weight each level by the inverse distance from the mid price
    /// (volume-weighted imbalance), otherwise use the plain top-N imbalance.
    #[builder(default = false)]
    pub weighted: bool,
    /// Rolling window (in timer evaluations) for the z-score mean/stddev.
    #[builder(default = 50)]
    pub zscore_window: usize,
    /// Z-score threshold above which a long (`> +`) or short (`< -`) position
    /// is opened.
    #[builder(default = 2.0)]
    pub entry_threshold: f64,
    /// Z-score magnitude below which an open position is reduced.
    #[builder(default = 0.5)]
    pub reduce_threshold: f64,
    /// Z-score magnitude below which an open position is fully closed.
    #[builder(default = 0.25)]
    pub close_threshold: f64,
    /// Capital (in quote currency) allocated to the strategy. When `None` it
    /// is resolved from the account equity during `on_start`.
    pub capital: Option<f64>,
    /// Notional per entry as a fraction of the allocated capital.
    #[builder(default = 0.10)]
    pub trade_size_pct: f64,
    /// Maximum net exposure as a fraction of the allocated capital.
    #[builder(default = 0.30)]
    pub max_position_pct: f64,
    /// Indicator evaluation cadence in milliseconds (timer-driven).
    #[builder(default = 1000)]
    pub timer_interval_ms: u64,
    /// Optional holding timeout in seconds after which open positions are closed.
    pub max_holding_secs: Option<u64>,
    /// When `true`, new entries are blocked while realized volatility is below
    /// its rolling median (low-vol regime).
    #[builder(default = false)]
    pub regime_filter_enabled: bool,
    /// Window (in timer evaluations) for the realized-volatility estimate.
    #[builder(default = 50)]
    pub regime_vol_window: usize,
    /// Window (in timer evaluations) over which the realized-volatility median
    /// is estimated.
    #[builder(default = 500)]
    pub regime_history_window: usize,
}

impl TryFrom<&ObiMomentumTomlConfig> for ObiMomentumConfig {
    type Error = anyhow::Error;

    fn try_from(cfg: &ObiMomentumTomlConfig) -> Result<Self, Self::Error> {
        Ok(Self::builder()
            .instrument_id(InstrumentId::from(cfg.instrument_id.as_str()))
            .num_levels(cfg.num_levels)
            .weighted(cfg.weighted)
            .zscore_window(cfg.zscore_window)
            .entry_threshold(cfg.entry_threshold)
            .reduce_threshold(cfg.reduce_threshold)
            .close_threshold(cfg.close_threshold)
            .maybe_capital(cfg.capital)
            .trade_size_pct(cfg.trade_size_pct)
            .max_position_pct(cfg.max_position_pct)
            .timer_interval_ms(cfg.timer_interval_ms)
            .maybe_max_holding_secs(cfg.max_holding_secs)
            .regime_filter_enabled(cfg.regime_filter_enabled)
            .regime_vol_window(cfg.regime_vol_window)
            .regime_history_window(cfg.regime_history_window)
            .build())
    }
}
