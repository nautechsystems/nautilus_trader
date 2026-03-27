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

use nautilus_model::identifiers::{ClientId, InstrumentId, StrategyId};

use crate::strategy::StrategyConfig;

/// Configuration for the delta-neutral short volatility hedger.
///
/// Tracks a short OTM call and put (strangle) and delta-hedges with the
/// underlying perpetual swap. Rehedges when portfolio delta exceeds a
/// configurable threshold or on a periodic timer.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
)]
pub struct DeltaNeutralVolConfig {
    /// Base strategy configuration.
    pub base: StrategyConfig,
    /// Option instrument family (e.g. "BTC-USD").
    pub option_family: String,
    /// Hedge instrument ID (e.g. BTC-USD-SWAP.OKX).
    pub hedge_instrument_id: InstrumentId,
    /// Data and execution client ID (e.g. "OKX").
    pub client_id: ClientId,
    /// Target call delta used by the startup strike heuristic.
    pub target_call_delta: f64,
    /// Target put delta used by the startup strike heuristic.
    pub target_put_delta: f64,
    /// Number of option contracts per leg.
    pub contracts: u64,
    /// Portfolio delta threshold that triggers a rehedge.
    pub rehedge_delta_threshold: f64,
    /// Periodic rehedge check interval in seconds.
    pub rehedge_interval_secs: u64,
    /// Optional expiry date filter (e.g. "260327").
    pub expiry_filter: Option<String>,
}

impl DeltaNeutralVolConfig {
    /// Creates a new [`DeltaNeutralVolConfig`] with required fields and defaults.
    #[must_use]
    pub fn new(
        option_family: String,
        hedge_instrument_id: InstrumentId,
        client_id: ClientId,
    ) -> Self {
        Self {
            base: StrategyConfig {
                strategy_id: Some(StrategyId::from("DELTA_NEUTRAL_VOL-001")),
                order_id_tag: Some("001".to_string()),
                ..Default::default()
            },
            option_family,
            hedge_instrument_id,
            client_id,
            target_call_delta: 0.20,
            target_put_delta: -0.20,
            contracts: 1,
            rehedge_delta_threshold: 0.5,
            rehedge_interval_secs: 30,
            expiry_filter: None,
        }
    }

    #[must_use]
    pub fn with_target_call_delta(mut self, delta: f64) -> Self {
        self.target_call_delta = delta;
        self
    }

    #[must_use]
    pub fn with_target_put_delta(mut self, delta: f64) -> Self {
        self.target_put_delta = delta;
        self
    }

    #[must_use]
    pub fn with_contracts(mut self, contracts: u64) -> Self {
        self.contracts = contracts;
        self
    }

    #[must_use]
    pub fn with_rehedge_delta_threshold(mut self, threshold: f64) -> Self {
        self.rehedge_delta_threshold = threshold;
        self
    }

    #[must_use]
    pub fn with_rehedge_interval_secs(mut self, secs: u64) -> Self {
        self.rehedge_interval_secs = secs;
        self
    }

    #[must_use]
    pub fn with_expiry_filter(mut self, expiry: String) -> Self {
        self.expiry_filter = Some(expiry);
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
impl DeltaNeutralVolConfig {
    #[new]
    #[pyo3(signature = (
        option_family,
        hedge_instrument_id,
        client_id,
        strategy_id=None,
        order_id_tag=None,
        target_call_delta=0.20,
        target_put_delta=-0.20,
        contracts=1,
        rehedge_delta_threshold=0.5,
        rehedge_interval_secs=30,
        expiry_filter=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        option_family: String,
        hedge_instrument_id: InstrumentId,
        client_id: ClientId,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        target_call_delta: f64,
        target_put_delta: f64,
        contracts: u64,
        rehedge_delta_threshold: f64,
        rehedge_interval_secs: u64,
        expiry_filter: Option<String>,
    ) -> Self {
        let mut config = Self::new(option_family, hedge_instrument_id, client_id)
            .with_target_call_delta(target_call_delta)
            .with_target_put_delta(target_put_delta)
            .with_contracts(contracts)
            .with_rehedge_delta_threshold(rehedge_delta_threshold)
            .with_rehedge_interval_secs(rehedge_interval_secs);

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        if let Some(expiry) = expiry_filter {
            config.expiry_filter = Some(expiry);
        }

        config
    }

    #[getter]
    fn option_family(&self) -> &str {
        &self.option_family
    }

    #[getter]
    fn hedge_instrument_id(&self) -> InstrumentId {
        self.hedge_instrument_id
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    #[getter]
    fn target_call_delta(&self) -> f64 {
        self.target_call_delta
    }

    #[getter]
    fn target_put_delta(&self) -> f64 {
        self.target_put_delta
    }

    #[getter]
    fn contracts(&self) -> u64 {
        self.contracts
    }

    #[getter]
    fn rehedge_delta_threshold(&self) -> f64 {
        self.rehedge_delta_threshold
    }

    #[getter]
    fn rehedge_interval_secs(&self) -> u64 {
        self.rehedge_interval_secs
    }

    #[getter]
    fn expiry_filter(&self) -> Option<&str> {
        self.expiry_filter.as_deref()
    }
}
