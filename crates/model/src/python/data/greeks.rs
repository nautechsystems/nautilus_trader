// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    BlackScholesGreeksResult, ImplyVolAndGreeksResult, black_scholes_greeks, imply_vol,
    imply_vol_and_greeks,
};

#[pymethods]
impl ImplyVolAndGreeksResult {
    /// Creates a new [`ImplyVolAndGreeksResult`] instance.
    #[new]
    fn py_new(vol: f64, price: f64, delta: f64, gamma: f64, theta: f64, vega: f64) -> Self {
        Self {
            vol,
            price,
            delta,
            gamma,
            theta,
            vega,
        }
    }

    #[getter]
    #[pyo3(name = "vol")]
    fn py_vol(&self) -> f64 {
        self.vol
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> f64 {
        self.price
    }

    #[getter]
    #[pyo3(name = "delta")]
    fn py_delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    #[pyo3(name = "gamma")]
    fn py_gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    #[pyo3(name = "vega")]
    fn py_vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    #[pyo3(name = "theta")]
    fn py_theta(&self) -> f64 {
        self.theta
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl BlackScholesGreeksResult {
    /// Creates a new [`BlackScholesGreeksResult`] instance.
    #[new]
    fn py_new(price: f64, delta: f64, gamma: f64, theta: f64, vega: f64) -> Self {
        Self {
            price,
            delta,
            gamma,
            theta,
            vega,
        }
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> f64 {
        self.price
    }

    #[getter]
    #[pyo3(name = "delta")]
    fn py_delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    #[pyo3(name = "gamma")]
    fn py_gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    #[pyo3(name = "vega")]
    fn py_vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    #[pyo3(name = "theta")]
    fn py_theta(&self) -> f64 {
        self.theta
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyfunction]
#[pyo3(name = "black_scholes_greeks")]
#[allow(clippy::too_many_arguments)]
pub fn py_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    sigma: f64,
    is_call: bool,
    k: f64,
    t: f64,
    multiplier: f64,
) -> PyResult<BlackScholesGreeksResult> {
    let result = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);
    Ok(result)
}

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
) -> PyResult<ImplyVolAndGreeksResult> {
    let result = imply_vol_and_greeks(s, r, b, is_call, k, t, price, multiplier);
    Ok(result)
}
