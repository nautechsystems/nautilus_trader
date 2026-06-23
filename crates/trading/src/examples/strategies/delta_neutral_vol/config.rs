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

//! Configuration for the delta-neutral volatility hedger.

use nautilus_model::{
    enums::TimeInForce,
    identifiers::{ClientId, InstrumentId, StrategyId},
};

use crate::strategy::StrategyConfig;

/// Configuration for the delta-neutral short volatility hedger.
///
/// Tracks a short OTM call and put (strangle) and delta-hedges with the
/// underlying perpetual swap. Rehedges when portfolio delta exceeds a
/// configurable threshold or on a periodic timer.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct DeltaNeutralVolConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("DELTA_NEUTRAL_VOL-001")),
        order_id_tag: Some("001".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    /// Option instrument family (e.g. "BTC-USD").
    pub option_family: String,
    /// Hedge instrument ID (e.g. BTC-USD-SWAP.OKX).
    pub hedge_instrument_id: InstrumentId,
    /// Data and execution client ID (e.g. "OKX").
    pub client_id: ClientId,
    /// Target call delta used by the startup strike heuristic.
    #[builder(default = 0.20)]
    pub target_call_delta: f64,
    /// Target put delta used by the startup strike heuristic.
    #[builder(default = -0.20)]
    pub target_put_delta: f64,
    /// Number of option contracts per leg.
    #[builder(default = 1)]
    pub contracts: u64,
    /// Portfolio delta threshold that triggers a rehedge.
    #[builder(default = 0.5)]
    pub rehedge_delta_threshold: f64,
    /// Periodic rehedge check interval in seconds.
    #[builder(default = 30)]
    pub rehedge_interval_secs: u64,
    /// Optional expiry date filter (e.g. "260327").
    pub expiry_filter: Option<String>,
    /// Place strangle entry orders when Greeks are first initialized.
    /// When false the strategy only hedges externally-entered positions.
    #[builder(default = true)]
    pub enter_strangle: bool,
    /// Implied volatility offset subtracted from mark IV for entry limit
    /// price. A value of 0.02 sells 2 vol points below mark (more aggressive).
    #[builder(default = 0.0)]
    pub entry_iv_offset: f64,
    /// Time-in-force for strangle entry orders.
    #[builder(default = TimeInForce::Gtc)]
    pub entry_time_in_force: TimeInForce,
    /// Tick offset from the option ask used for premium-priced entry orders.
    /// When set, the strategy does not pass IV params to the adapter.
    pub entry_premium_offset_ticks: Option<i32>,
    /// Param key for implied volatility passed to `submit_order`.
    /// Adapter-specific: Bybit uses `"order_iv"`, OKX uses `"px_vol"`.
    #[builder(default = "px_vol".to_string())]
    pub iv_param_key: String,
}
