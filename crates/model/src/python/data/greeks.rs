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

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::{prelude::*, types::PyType};

use crate::data::greeks::{
    BlackScholesGreeksResult, GreeksData, OptionGreekValues, PortfolioGreeks, YieldCurve,
    black_scholes_greeks_checked, imply_vol_and_greeks_checked, imply_vol_checked,
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

    /// Returns portfolio values with `pnl`, `price`, and all Greeks scaled by the contract multiplier.
    ///
    /// Preserves both timestamps and leaves position quantity scaling to the caller.
    #[pyo3(name = "to_portfolio_greeks")]
    fn py_to_portfolio_greeks(&self) -> PortfolioGreeks {
        self.to_portfolio_greeks()
    }

    fn __rmul__(&self, scalar: f64) -> PortfolioGreeks {
        scalar * &self.to_portfolio_greeks()
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

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioGreeks {
    fn __rmul__(&self, scalar: f64) -> Self {
        scalar * self
    }

    fn __add__(&self, other: &Self) -> Self {
        self.clone() + other.clone()
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
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl YieldCurve {
    #[new]
    fn py_new(
        curve_name: String,
        tenors: Vec<f64>,
        interest_rates: Vec<f64>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        let curve = Self::new(
            curve_name,
            tenors,
            interest_rates,
            ts_event.into(),
            ts_init.into(),
        );
        curve.validate().map_err(to_pyvalue_err)?;
        Ok(curve)
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn curve_name(&self) -> &str {
        &self.curve_name
    }

    #[getter]
    fn tenors(&self) -> Vec<f64> {
        self.tenors.clone()
    }

    #[getter]
    fn interest_rates(&self) -> Vec<f64> {
        self.interest_rates.clone()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
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

    #[getter]
    fn rho(&self) -> f64 {
        self.rho
    }
}

/// Computes Black-Scholes greeks using the fast `compute_greeks` implementation.
/// This function uses `compute_greeks` from `black_scholes.rs` which is optimized for performance.
#[doc = "

`db_dr` is zero for fixed carry and one for carry defined as `r - q`, including `r == q`.
Rho is per percentage point.
The exact implementation is used when the fast result is nonfinite or inconsistent.

# Errors

Returns an error if:
- Model inputs are invalid.
- Valuation results are nonfinite or inconsistent."]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "black_scholes_greeks", signature = (s, r, b, vol, is_call, k, t, db_dr=0.0))]
#[expect(clippy::too_many_arguments)]
pub fn py_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
    db_dr: f64,
) -> PyResult<BlackScholesGreeksResult> {
    black_scholes_greeks_checked(s, r, b, vol, is_call, k, t, db_dr).map_err(to_pyvalue_err)
}

/// Computes the implied volatility for an option given its parameters and market price.
///
/// # Errors
///
/// Returns an error if implied volatility calculation fails.
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
    imply_vol_checked(s, r, b, is_call, k, t, price).map_err(to_pyvalue_err)
}

/// Computes implied volatility and greeks using the fast implementations.
/// This function uses `compute_greeks` after implying volatility.
#[doc = "

`db_dr` is zero for fixed carry and one for carry defined as `r - q`, including `r == q`.
Rho is per percentage point.
The exact implementation is used when the fast kernel cannot reprice the solved premium
within 0.01 percent.

# Errors

Returns an error if:
- Model inputs or the premium are invalid.
- The premium has no finite positive implied volatility.
- Results are nonfinite, inconsistent, or fail the 0.01 percent repricing tolerance."]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "imply_vol_and_greeks", signature = (s, r, b, is_call, k, t, price, db_dr=0.0))]
#[expect(clippy::too_many_arguments)]
pub fn py_imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
    db_dr: f64,
) -> PyResult<BlackScholesGreeksResult> {
    imply_vol_and_greeks_checked(s, r, b, is_call, k, t, price, db_dr, None).map_err(to_pyvalue_err)
}

/// Refines implied volatility using an initial guess and computes greeks.
/// This function uses `compute_iv_and_greeks` which performs a Halley iteration
/// to refine the volatility estimate from an initial guess.
#[doc = "

`db_dr` is zero for fixed carry and one for carry defined as `r - q`, including `r == q`.
Rho is per percentage point.

Refinement falls back to the full implied-volatility solver when its volatility reaches a
clamp bound or it fails to reprice the premium within 0.01 percent.
The exact implementation is used when the fast kernel cannot reprice the solved premium
within that tolerance.

# Errors

Returns an error if:
- Model inputs or the premium are invalid.
- The premium has no finite positive implied volatility.
- Results are nonfinite, inconsistent, or fail the 0.01 percent repricing tolerance."]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "refine_vol_and_greeks", signature = (s, r, b, is_call, k, t, target_price, initial_vol, db_dr=0.0))]
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
    db_dr: f64,
) -> PyResult<BlackScholesGreeksResult> {
    imply_vol_and_greeks_checked(
        s,
        r,
        b,
        is_call,
        k,
        t,
        target_price,
        db_dr,
        Some(initial_vol),
    )
    .map_err(to_pyvalue_err)
}
