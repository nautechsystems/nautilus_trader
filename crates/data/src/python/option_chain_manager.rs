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

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
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

fn parse_option_kind(value: u8) -> PyResult<OptionKind> {
    match value {
        0 => Ok(OptionKind::Call),
        1 => Ok(OptionKind::Put),
        _ => Err(to_pyvalue_err(format!(
            "invalid `OptionKind` value, expected 0 (Call) or 1 (Put), received {value}"
        ))),
    }
}

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
#[pyclass(
    name = "OptionChainManager",
    module = "nautilus_trader.core.nautilus_pyo3.data"
)]
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
    ) -> PyResult<Self> {
        let rust_instruments: HashMap<InstrumentId, (Price, OptionKind)> = instruments
            .into_iter()
            .map(|(id, (strike, kind_u8))| {
                parse_option_kind(kind_u8).map(|kind| (id, (strike, kind)))
            })
            .collect::<PyResult<_>>()?;

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

        Ok(Self {
            aggregator,
            series_id,
            raw_mode,
            bootstrapped,
        })
    }

    /// Feeds a quote tick to the aggregator.
    ///
    /// Returns `True` if the manager just bootstrapped (first ATM price arrived
    /// and the active instrument set was computed for the first time).
    #[pyo3(name = "handle_quote")]
    fn py_handle_quote(&mut self, quote: &Bound<'_, PyAny>) -> PyResult<bool> {
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
    #[pyo3(name = "handle_greeks")]
    fn py_handle_greeks(&mut self, greeks_obj: &Bound<'_, PyAny>) -> PyResult<bool> {
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
    #[pyo3(name = "snapshot")]
    fn py_snapshot(&self, ts_ns: u64) -> Option<OptionChainSlice> {
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
    #[pyo3(name = "check_rebalance")]
    fn py_check_rebalance(&mut self, ts_ns: u64) -> Option<(Vec<InstrumentId>, Vec<InstrumentId>)> {
        let now = UnixNanos::from(ts_ns);
        let action = self.aggregator.check_rebalance(now)?;
        let add = action.add.clone();
        let remove = action.remove.clone();
        self.aggregator.apply_rebalance(&action, now);
        Some((add, remove))
    }

    /// Returns the currently active instrument IDs (the subset being tracked).
    #[pyo3(name = "active_instrument_ids")]
    fn py_active_instrument_ids(&self) -> Vec<InstrumentId> {
        self.aggregator.instrument_ids()
    }

    /// Returns all instrument IDs in the full catalog.
    #[pyo3(name = "all_instrument_ids")]
    fn py_all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.aggregator.all_instrument_ids()
    }

    /// Adds a newly discovered instrument to the series.
    ///
    /// Returns `True` if the instrument was newly inserted.
    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(
        &mut self,
        instrument_id: InstrumentId,
        strike: Price,
        kind: u8,
    ) -> PyResult<bool> {
        let option_kind = parse_option_kind(kind)?;
        Ok(self
            .aggregator
            .add_instrument(instrument_id, strike, option_kind))
    }

    /// Removes an instrument from the catalog.
    ///
    /// Returns `True` if the catalog is now empty.
    #[pyo3(name = "remove_instrument")]
    fn py_remove_instrument(&mut self, instrument_id: InstrumentId) -> bool {
        let _ = self.aggregator.remove_instrument(&instrument_id);
        self.aggregator.is_catalog_empty()
    }

    #[getter]
    #[pyo3(name = "series_id")]
    fn py_series_id(&self) -> OptionSeriesId {
        self.series_id
    }

    #[getter]
    #[pyo3(name = "bootstrapped")]
    fn py_bootstrapped(&self) -> bool {
        self.bootstrapped
    }

    #[getter]
    #[pyo3(name = "raw_mode")]
    fn py_raw_mode(&self) -> bool {
        self.raw_mode
    }

    #[getter]
    #[pyo3(name = "atm_price")]
    fn py_atm_price(&self) -> Option<Price> {
        self.aggregator.atm_tracker().atm_price()
    }

    fn __repr__(&self) -> String {
        format!(
            "OptionChainManager(series_id={}, bootstrapped={}, raw_mode={}, \
             active={}/{})",
            self.series_id,
            self.bootstrapped,
            self.raw_mode,
            self.aggregator.instrument_ids().len(),
            self.aggregator.all_instrument_ids().len(),
        )
    }
}
