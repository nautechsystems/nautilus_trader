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

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;
use pyo3::prelude::*;

use crate::{
    data::{
        QuoteTick,
        greeks::OptionGreekValues,
        option_chain::{OptionChainSlice, OptionGreeks, OptionStrikeData, StrikeRange},
    },
    enums::GreeksConvention,
    identifiers::{InstrumentId, OptionSeriesId},
    types::Price,
};

/// Python wrapper for `StrikeRange` (complex enum).
#[pyclass(
    name = "StrikeRange",
    module = "nautilus_trader.core.nautilus_pyo3.model",
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
#[derive(Clone, Debug)]
pub struct PyStrikeRange {
    pub inner: StrikeRange,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyStrikeRange {
    /// Creates a `StrikeRange::Fixed` variant.
    #[staticmethod]
    #[pyo3(name = "fixed")]
    fn py_fixed(strikes: Vec<Price>) -> Self {
        Self {
            inner: StrikeRange::Fixed(strikes),
        }
    }

    /// Creates a `StrikeRange::AtmRelative` variant.
    #[staticmethod]
    #[pyo3(name = "atm_relative")]
    fn py_atm_relative(strikes_above: usize, strikes_below: usize) -> Self {
        Self {
            inner: StrikeRange::AtmRelative {
                strikes_above,
                strikes_below,
            },
        }
    }

    /// Creates a `StrikeRange::AtmPercent` variant.
    #[staticmethod]
    #[pyo3(name = "atm_percent")]
    fn py_atm_percent(pct: f64) -> Self {
        Self {
            inner: StrikeRange::AtmPercent { pct },
        }
    }

    /// Returns the variant name (`Fixed`, `AtmRelative`, or `AtmPercent`).
    #[getter]
    #[pyo3(name = "kind")]
    fn py_kind(&self) -> &'static str {
        match self.inner {
            StrikeRange::Fixed(_) => "Fixed",
            StrikeRange::AtmRelative { .. } => "AtmRelative",
            StrikeRange::AtmPercent { .. } => "AtmPercent",
        }
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }

    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OptionGreeks {
    /// Exchange-provided option Greeks and implied volatility for a single instrument.
    #[new]
    #[pyo3(signature = (instrument_id, delta, gamma, vega, theta, rho=0.0, mark_iv=None, bid_iv=None, ask_iv=None, underlying_price=None, open_interest=None, ts_event=0, ts_init=0, convention=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        delta: f64,
        gamma: f64,
        vega: f64,
        theta: f64,
        rho: f64,
        mark_iv: Option<f64>,
        bid_iv: Option<f64>,
        ask_iv: Option<f64>,
        underlying_price: Option<f64>,
        open_interest: Option<f64>,
        ts_event: u64,
        ts_init: u64,
        convention: Option<GreeksConvention>,
    ) -> Self {
        Self {
            instrument_id,
            convention: convention.unwrap_or_default(),
            greeks: OptionGreekValues {
                delta,
                gamma,
                vega,
                theta,
                rho,
            },
            mark_iv,
            bid_iv,
            ask_iv,
            underlying_price,
            open_interest,
            ts_event: UnixNanos::from(ts_event),
            ts_init: UnixNanos::from(ts_init),
        }
    }

    #[getter]
    #[pyo3(name = "convention")]
    fn py_convention(&self) -> GreeksConvention {
        self.convention
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "delta")]
    fn py_delta(&self) -> f64 {
        self.greeks.delta
    }

    #[getter]
    #[pyo3(name = "gamma")]
    fn py_gamma(&self) -> f64 {
        self.greeks.gamma
    }

    #[getter]
    #[pyo3(name = "vega")]
    fn py_vega(&self) -> f64 {
        self.greeks.vega
    }

    #[getter]
    #[pyo3(name = "theta")]
    fn py_theta(&self) -> f64 {
        self.greeks.theta
    }

    #[getter]
    #[pyo3(name = "rho")]
    fn py_rho(&self) -> f64 {
        self.greeks.rho
    }

    #[getter]
    #[pyo3(name = "mark_iv")]
    fn py_mark_iv(&self) -> Option<f64> {
        self.mark_iv
    }

    #[getter]
    #[pyo3(name = "bid_iv")]
    fn py_bid_iv(&self) -> Option<f64> {
        self.bid_iv
    }

    #[getter]
    #[pyo3(name = "ask_iv")]
    fn py_ask_iv(&self) -> Option<f64> {
        self.ask_iv
    }

    #[getter]
    #[pyo3(name = "underlying_price")]
    fn py_underlying_price(&self) -> Option<f64> {
        self.underlying_price
    }

    #[getter]
    #[pyo3(name = "open_interest")]
    fn py_open_interest(&self) -> Option<f64> {
        self.open_interest
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    fn __repr__(&self) -> String {
        format!("{self}")
    }

    fn __str__(&self) -> String {
        format!("{self}")
    }
}

impl OptionGreeks {
    /// Creates an `OptionGreeks` from a Python object.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python object is missing required attributes.
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id = obj.getattr("instrument_id")?.extract::<InstrumentId>()?;
        let delta = obj.getattr("delta")?.extract::<f64>()?;
        let gamma = obj.getattr("gamma")?.extract::<f64>()?;
        let vega = obj.getattr("vega")?.extract::<f64>()?;
        let theta = obj.getattr("theta")?.extract::<f64>()?;
        let rho = obj.getattr("rho")?.extract::<f64>()?;
        let mark_iv = obj.getattr("mark_iv")?.extract::<Option<f64>>()?;
        let bid_iv = obj.getattr("bid_iv")?.extract::<Option<f64>>()?;
        let ask_iv = obj.getattr("ask_iv")?.extract::<Option<f64>>()?;
        let underlying_price = obj.getattr("underlying_price")?.extract::<Option<f64>>()?;
        let open_interest = obj.getattr("open_interest")?.extract::<Option<f64>>()?;
        let ts_event = obj.getattr("ts_event")?.extract::<u64>()?;
        let ts_init = obj.getattr("ts_init")?.extract::<u64>()?;
        let convention = obj
            .getattr("convention")
            .ok()
            .and_then(|v| v.extract::<GreeksConvention>().ok())
            .unwrap_or_default();

        Ok(Self {
            instrument_id,
            convention,
            greeks: OptionGreekValues {
                delta,
                gamma,
                vega,
                theta,
                rho,
            },
            mark_iv,
            bid_iv,
            ask_iv,
            underlying_price,
            open_interest,
            ts_event: UnixNanos::from(ts_event),
            ts_init: UnixNanos::from(ts_init),
        })
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OptionStrikeData {
    /// Combined quote and Greeks data for a single strike in an option chain.
    #[new]
    #[pyo3(signature = (quote, greeks=None))]
    fn py_new(quote: QuoteTick, greeks: Option<OptionGreeks>) -> Self {
        Self { quote, greeks }
    }

    #[getter]
    #[pyo3(name = "quote")]
    fn py_quote(&self) -> QuoteTick {
        self.quote
    }

    #[getter]
    #[pyo3(name = "greeks")]
    fn py_greeks(&self) -> Option<OptionGreeks> {
        self.greeks
    }

    fn __repr__(&self) -> String {
        format!(
            "OptionStrikeData(quote={}, greeks={:?})",
            self.quote, self.greeks
        )
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OptionChainSlice {
    /// A point-in-time snapshot of an option chain for a single series.
    #[new]
    #[pyo3(signature = (series_id, atm_strike=None, ts_event=0, ts_init=0))]
    fn py_new(
        series_id: OptionSeriesId,
        atm_strike: Option<Price>,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self {
            series_id,
            atm_strike,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            ts_event: UnixNanos::from(ts_event),
            ts_init: UnixNanos::from(ts_init),
        }
    }

    #[getter]
    #[pyo3(name = "series_id")]
    fn py_series_id(&self) -> OptionSeriesId {
        self.series_id
    }

    #[getter]
    #[pyo3(name = "atm_strike")]
    fn py_atm_strike(&self) -> Option<Price> {
        self.atm_strike
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    /// Returns the number of call entries.
    #[pyo3(name = "call_count")]
    fn py_call_count(&self) -> usize {
        self.call_count()
    }

    /// Returns the number of put entries.
    #[pyo3(name = "put_count")]
    fn py_put_count(&self) -> usize {
        self.put_count()
    }

    /// Returns the total number of unique strikes.
    #[pyo3(name = "strike_count")]
    fn py_strike_count(&self) -> usize {
        self.strike_count()
    }

    /// Returns `true` if the chain has no data.
    #[pyo3(name = "is_empty")]
    fn py_is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Returns all strike prices present in the chain (union of calls and puts).
    #[pyo3(name = "strikes")]
    fn py_strikes(&self) -> Vec<Price> {
        self.strikes()
    }

    /// Returns the call data for a given strike price.
    #[pyo3(name = "get_call")]
    fn py_get_call(&self, strike: Price) -> Option<OptionStrikeData> {
        self.get_call(&strike).cloned()
    }

    /// Returns the put data for a given strike price.
    #[pyo3(name = "get_put")]
    fn py_get_put(&self, strike: Price) -> Option<OptionStrikeData> {
        self.get_put(&strike).cloned()
    }

    /// Returns the call quote for a given strike price.
    #[pyo3(name = "get_call_quote")]
    fn py_get_call_quote(&self, strike: Price) -> Option<QuoteTick> {
        self.get_call_quote(&strike).copied()
    }

    /// Returns the put quote for a given strike price.
    #[pyo3(name = "get_put_quote")]
    fn py_get_put_quote(&self, strike: Price) -> Option<QuoteTick> {
        self.get_put_quote(&strike).copied()
    }

    /// Returns the call Greeks for a given strike price.
    #[pyo3(name = "get_call_greeks")]
    fn py_get_call_greeks(&self, strike: Price) -> Option<OptionGreeks> {
        self.get_call_greeks(&strike).copied()
    }

    /// Returns the put Greeks for a given strike price.
    #[pyo3(name = "get_put_greeks")]
    fn py_get_put_greeks(&self, strike: Price) -> Option<OptionGreeks> {
        self.get_put_greeks(&strike).copied()
    }

    fn __repr__(&self) -> String {
        format!("{self}")
    }

    fn __str__(&self) -> String {
        format!("{self}")
    }
}
