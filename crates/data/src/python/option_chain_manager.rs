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

//! PyO3 wrapper for the option chain aggregation engine.
//!
//! [`PyOptionChainManager`] wraps [`OptionChainAggregator`] and [`AtmTracker`],
//! exposing them to the Cython `DataEngine` without Rust msgbus, clock, or timer
//! dependencies. The Cython engine owns the lifecycle: subscription routing,
//! timer management, and msgbus publishing.

use std::collections::HashMap;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        QuoteTick,
        option_chain::{OptionChainSlice, OptionGreeks},
    },
    enums::OptionKind,
    identifiers::{InstrumentId, OptionSeriesId},
    python::data::option_chain::PyStrikeRange,
    types::Price,
};
use pyo3::prelude::*;

use crate::option_chains::{AtmTracker, OptionChainAggregator};

// ---------------------------------------------------------------------------
// PyOptionChainManager
// ---------------------------------------------------------------------------

/// Python-facing option chain manager that wraps [`OptionChainAggregator`] and
/// [`AtmTracker`].
///
/// The Cython `DataEngine` creates one instance per subscribed option series.
/// It feeds incoming market data (quotes, greeks) through the `handle_*`
/// methods and periodically calls `snapshot()` to publish `OptionChainSlice`
/// objects to the message bus.
///
/// ATM price is always derived from the exchange-provided forward price
/// embedded in each option greeks/ticker update.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.data")]
#[derive(Debug)]
pub struct PyOptionChainManager {
    aggregator: OptionChainAggregator,
    series_id: OptionSeriesId,
    raw_mode: bool,
    bootstrapped: bool,
}

#[pymethods]
impl PyOptionChainManager {
    /// Creates a new option chain manager.
    #[new]
    #[pyo3(signature = (series_id, strike_range, instruments, snapshot_interval_ms=None, initial_atm_price=None))]
    fn py_new(
        series_id: OptionSeriesId,
        strike_range: PyStrikeRange,
        instruments: HashMap<InstrumentId, (Price, u8)>,
        snapshot_interval_ms: Option<u64>,
        initial_atm_price: Option<Price>,
    ) -> Self {
        let rust_instruments: HashMap<InstrumentId, (Price, OptionKind)> = instruments
            .into_iter()
            .map(|(id, (strike, kind_u8))| {
                let kind = if kind_u8 == 0 {
                    OptionKind::Call
                } else {
                    OptionKind::Put
                };
                (id, (strike, kind))
            })
            .collect();

        let mut tracker = AtmTracker::new();

        // Derive precision from instrument strikes
        if let Some((strike, _)) = rust_instruments.values().next() {
            tracker.set_forward_precision(strike.precision);
        }

        if let Some(price) = initial_atm_price {
            tracker.set_initial_price(price);
        }

        let aggregator =
            OptionChainAggregator::new(series_id, strike_range.inner, tracker, rust_instruments);

        let active_ids = aggregator.instrument_ids();
        let all_ids = aggregator.all_instrument_ids();
        let bootstrapped = !active_ids.is_empty() || all_ids.is_empty();
        let raw_mode = snapshot_interval_ms.is_none();

        Self {
            aggregator,
            series_id,
            raw_mode,
            bootstrapped,
        }
    }

    /// Feeds a quote tick to the aggregator.
    ///
    /// Returns `True` if the manager just bootstrapped (first ATM price arrived
    /// and the active instrument set was computed for the first time).
    fn handle_quote(&mut self, quote: &Bound<'_, PyAny>) -> PyResult<bool> {
        let tick = quote
            .extract::<QuoteTick>()
            .or_else(|_| QuoteTick::from_pyobject(quote))?;
        self.aggregator.update_quote(&tick);

        if !self.bootstrapped && self.aggregator.atm_tracker().atm_price().is_some() {
            self.aggregator.recompute_active_set();
            self.bootstrapped = true;
            return Ok(true);
        }
        Ok(false)
    }

    /// Feeds option greeks to the aggregator.
    ///
    /// Returns `True` if the manager just bootstrapped (ATM price derived from
    /// the greeks' `underlying_price`).
    fn handle_greeks(&mut self, greeks_obj: &Bound<'_, PyAny>) -> PyResult<bool> {
        let greeks = greeks_obj
            .extract::<OptionGreeks>()
            .or_else(|_| OptionGreeks::from_pyobject(greeks_obj))?;

        // Update ATM tracker from greeks forward price
        self.aggregator
            .atm_tracker_mut()
            .update_from_option_greeks(&greeks);

        // Update aggregator buffers
        self.aggregator.update_greeks(&greeks);

        if !self.bootstrapped && self.aggregator.atm_tracker().atm_price().is_some() {
            self.aggregator.recompute_active_set();
            self.bootstrapped = true;
            return Ok(true);
        }
        Ok(false)
    }

    /// Creates a point-in-time snapshot from accumulated buffers.
    ///
    /// Returns `None` if no data has been accumulated yet (both buffers empty).
    fn snapshot(&self, ts_ns: u64) -> Option<OptionChainSlice> {
        if self.aggregator.is_buffer_empty() {
            return None;
        }
        Some(self.aggregator.snapshot(UnixNanos::from(ts_ns)))
    }

    /// Checks whether instruments should be rebalanced around the current ATM.
    ///
    /// Returns `None` when no rebalancing is needed.
    /// Returns `(added_ids, removed_ids)` when the active set should change.
    /// The caller is responsible for subscribing/unsubscribing instruments.
    fn check_rebalance(&mut self, ts_ns: u64) -> Option<(Vec<InstrumentId>, Vec<InstrumentId>)> {
        let now = UnixNanos::from(ts_ns);
        let action = self.aggregator.check_rebalance(now)?;
        let add = action.add.clone();
        let remove = action.remove.clone();
        self.aggregator.apply_rebalance(&action, now);
        Some((add, remove))
    }

    /// Returns the currently active instrument IDs (the subset being tracked).
    fn active_instrument_ids(&self) -> Vec<InstrumentId> {
        self.aggregator.instrument_ids()
    }

    /// Returns all instrument IDs in the full catalog.
    fn all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.aggregator.all_instrument_ids()
    }

    /// Adds a newly discovered instrument to the series.
    ///
    /// Returns `True` if the instrument was newly inserted.
    fn add_instrument(&mut self, instrument_id: InstrumentId, strike: Price, kind: u8) -> bool {
        let option_kind = if kind == 0 {
            OptionKind::Call
        } else {
            OptionKind::Put
        };
        self.aggregator
            .add_instrument(instrument_id, strike, option_kind)
    }

    /// Removes an instrument from the catalog.
    ///
    /// Returns `True` if the catalog is now empty.
    fn remove_instrument(&mut self, instrument_id: InstrumentId) -> bool {
        let _ = self.aggregator.remove_instrument(&instrument_id);
        self.aggregator.is_catalog_empty()
    }

    #[getter]
    fn series_id(&self) -> OptionSeriesId {
        self.series_id
    }

    #[getter]
    fn bootstrapped(&self) -> bool {
        self.bootstrapped
    }

    #[getter]
    fn raw_mode(&self) -> bool {
        self.raw_mode
    }

    #[getter]
    fn atm_price(&self) -> Option<Price> {
        self.aggregator.atm_tracker().atm_price()
    }

    fn __repr__(&self) -> String {
        format!(
            "PyOptionChainManager(series_id={}, bootstrapped={}, raw_mode={}, \
             active={}/{})",
            self.series_id,
            self.bootstrapped,
            self.raw_mode,
            self.aggregator.instrument_ids().len(),
            self.aggregator.all_instrument_ids().len(),
        )
    }
}
