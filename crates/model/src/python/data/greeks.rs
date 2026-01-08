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

use pyo3::prelude::*;

use crate::data::greeks::{
    BlackScholesGreeksResult, black_scholes_greeks, imply_vol, imply_vol_and_greeks,
    refine_vol_and_greeks,
};

#[cfg(feature = "python")]
#[pymethods]
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
}

/// Computes Black-Scholes greeks for given parameters using the fast compute_greeks implementation.
///
/// # Errors
///
/// Returns a `PyErr` if the greeks calculation fails.
#[pyfunction]
#[pyo3(name = "black_scholes_greeks")]
#[allow(clippy::too_many_arguments)]
pub fn py_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
    multiplier: f64,
) -> PyResult<BlackScholesGreeksResult> {
    Ok(black_scholes_greeks(
        s, r, b, vol, is_call, k, t, multiplier,
    ))
}

/// Computes the implied volatility for an option given its parameters and market price.
///
/// # Errors
///
/// Returns a `PyErr` if implied volatility calculation fails.
#[pyfunction]
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

/// Computes implied volatility and option greeks for given parameters and market price.
/// This function uses compute_greeks after implying volatility.
///
/// # Errors
///
/// Returns a `PyErr` if calculation fails.
#[pyfunction]
#[pyo3(name = "imply_vol_and_greeks")]
#[allow(clippy::too_many_arguments)]
pub fn py_imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
    multiplier: f64,
) -> PyResult<BlackScholesGreeksResult> {
    Ok(imply_vol_and_greeks(
        s, r, b, is_call, k, t, price, multiplier,
    ))
}

/// Refines implied volatility using an initial guess and computes greeks.
/// This function uses compute_iv_and_greeks which performs a Halley iteration
/// to refine the volatility estimate from an initial guess.
///
/// # Errors
///
/// Returns a `PyErr` if calculation fails.
#[pyfunction]
#[pyo3(name = "refine_vol_and_greeks")]
#[allow(clippy::too_many_arguments)]
pub fn py_refine_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    target_price: f64,
    initial_vol: f64,
    multiplier: f64,
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
        multiplier,
    ))
}
