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

use nautilus_core::UnixNanos;
use pyo3::{prelude::*, types::PyType};

use crate::data::greeks::{
    BlackScholesGreeksResult, GreeksData, OptionGreekValues, PortfolioGreeks, black_scholes_greeks,
    imply_vol, imply_vol_and_greeks, refine_vol_and_greeks,
};

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OptionGreekValues {
    #[getter]
    fn delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.rho
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GreeksData {
    #[classmethod]
    #[pyo3(name = "from_delta", signature = (instrument_id, delta, multiplier, ts_event=0))]
    fn py_from_delta(
        _cls: &Bound<'_, PyType>,
        instrument_id: crate::identifiers::InstrumentId,
        delta: f64,
        multiplier: f64,
        ts_event: u64,
    ) -> Self {
        Self::from_delta(instrument_id, delta, multiplier, UnixNanos::from(ts_event))
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn instrument_id(&self) -> crate::identifiers::InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn is_call(&self) -> bool {
        self.is_call
    }

    #[getter]
    fn strike(&self) -> f64 {
        self.strike
    }

    #[getter]
    fn expiry(&self) -> i32 {
        self.expiry
    }

    #[getter]
    fn expiry_in_days(&self) -> i32 {
        self.expiry_in_days
    }

    #[getter]
    fn expiry_in_years(&self) -> f64 {
        self.expiry_in_years
    }

    #[getter]
    fn multiplier(&self) -> f64 {
        self.multiplier
    }

    #[getter]
    fn quantity(&self) -> f64 {
        self.quantity
    }

    #[getter]
    fn underlying_price(&self) -> f64 {
        self.underlying_price
    }

    #[getter]
    fn interest_rate(&self) -> f64 {
        self.interest_rate
    }

    #[getter]
    fn cost_of_carry(&self) -> f64 {
        self.cost_of_carry
    }

    #[getter]
    fn vol(&self) -> f64 {
        self.vol
    }

    #[getter]
    fn pnl(&self) -> f64 {
        self.pnl
    }

    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.greeks.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.greeks.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.greeks.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.greeks.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.greeks.rho
    }

    #[getter]
    fn itm_prob(&self) -> f64 {
        self.itm_prob
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioGreeks {
    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn pnl(&self) -> f64 {
        self.pnl
    }

    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.greeks.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.greeks.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.greeks.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.greeks.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.greeks.rho
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BlackScholesGreeksResult {
    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn vol(&self) -> f64 {
        self.vol
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.theta
    }

    #[getter]
    fn itm_prob(&self) -> f64 {
        self.itm_prob
    }
}

/// Computes Black-Scholes greeks using the fast `compute_greeks` implementation.
/// This function uses `compute_greeks` from `black_scholes.rs` which is optimized for performance.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "black_scholes_greeks")]
pub fn py_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
) -> PyResult<BlackScholesGreeksResult> {
    Ok(black_scholes_greeks(s, r, b, vol, is_call, k, t))
}

/// Computes the implied volatility for an option given its parameters and market price.
///
/// # Errors
///
/// Returns a `PyErr` if implied volatility calculation fails.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "imply_vol")]
pub fn py_imply_vol(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> PyResult<f64> {
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    Ok(vol)
}

/// Computes implied volatility and greeks using the fast implementations.
/// This function uses `compute_greeks` after implying volatility.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "imply_vol_and_greeks")]
pub fn py_imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> PyResult<BlackScholesGreeksResult> {
    Ok(imply_vol_and_greeks(s, r, b, is_call, k, t, price))
}

/// Refines implied volatility using an initial guess and computes greeks.
/// This function uses `compute_iv_and_greeks` which performs a Halley iteration
/// to refine the volatility estimate from an initial guess.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "refine_vol_and_greeks")]
#[expect(clippy::too_many_arguments)]
pub fn py_refine_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    target_price: f64,
    initial_vol: f64,
) -> PyResult<BlackScholesGreeksResult> {
    Ok(refine_vol_and_greeks(
        s,
        r,
        b,
        is_call,
        k,
        t,
        target_price,
        initial_vol,
    ))
}
