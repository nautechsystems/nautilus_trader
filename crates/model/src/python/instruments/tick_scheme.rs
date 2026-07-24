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

//! Python bindings for named tick schemes.

use nautilus_core::python::{to_pytype_err, to_pyvalue_err};
use pyo3::{
    Py, PyAny, PyResult, Python,
    prelude::{Bound, PyRef},
    types::PyAnyMethods,
};

use crate::{
    instruments::{
        FixedTickScheme, TickScheme, TickSchemeRule, TieredTickScheme, get_tick_scheme,
        list_tick_schemes, register_tick_scheme,
        tick_scheme::{BETFAIR_TICK_SCHEME, CRYPTO_0_01_TICK_SCHEME_NAME},
    },
    types::Price,
};

#[derive(Clone, Debug)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
#[pyo3::pyclass(
    frozen,
    name = "FixedTickScheme",
    module = "nautilus_trader.core.nautilus_pyo3.model",
    skip_from_py_object
)]
pub struct PyFixedTickScheme {
    name: String,
    inner: FixedTickScheme,
}

impl PyFixedTickScheme {
    fn from_registered(name: String, inner: FixedTickScheme) -> Self {
        Self { name, inner }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl PyFixedTickScheme {
    #[new]
    #[pyo3(signature = (name, price_precision, increment=None))]
    fn py_new(name: String, price_precision: u8, increment: Option<f64>) -> PyResult<Self> {
        let increment = increment.unwrap_or_else(|| 10_f64.powi(-i32::from(price_precision)));
        let inner = FixedTickScheme::new_with_precision(increment, price_precision)
            .map_err(to_pyvalue_err)?;
        Ok(Self { name, inner })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.inner.precision()
    }

    #[getter]
    fn increment(&self) -> Price {
        Price::new(self.inner.tick(), self.inner.precision())
    }

    #[pyo3(signature = (value, n=0))]
    fn next_bid_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self.inner.next_bid_price(value, n, self.inner.precision()))
    }

    #[pyo3(signature = (value, n=0))]
    fn next_ask_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self.inner.next_ask_price(value, n, self.inner.precision()))
    }
}

#[derive(Clone, Debug)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
#[pyo3::pyclass(
    frozen,
    name = "TieredTickScheme",
    module = "nautilus_trader.core.nautilus_pyo3.model",
    skip_from_py_object
)]
pub struct PyTieredTickScheme {
    name: String,
    inner: TieredTickScheme,
}

impl PyTieredTickScheme {
    fn from_registered(name: String, inner: TieredTickScheme) -> Self {
        Self { name, inner }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl PyTieredTickScheme {
    #[new]
    #[pyo3(signature = (name, tiers, price_precision, max_ticks_per_tier=100))]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 constructors receive owned Python arguments"
    )]
    fn py_new(
        name: String,
        tiers: Vec<(f64, f64, f64)>,
        price_precision: u8,
        max_ticks_per_tier: usize,
    ) -> PyResult<Self> {
        let inner = TieredTickScheme::new(&tiers, price_precision, max_ticks_per_tier)
            .map_err(to_pyvalue_err)?;
        Ok(Self { name, inner })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.inner.precision()
    }

    #[getter]
    fn min_price(&self) -> Price {
        self.inner.min_price()
    }

    #[getter]
    fn max_price(&self) -> Price {
        self.inner.max_price()
    }

    #[getter]
    fn ticks(&self) -> Vec<Price> {
        self.inner.ticks()
    }

    #[getter]
    fn tick_count(&self) -> usize {
        self.inner.tick_count()
    }

    #[pyo3(signature = (value, n=0))]
    fn next_bid_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self.inner.next_bid_price(value, n, self.inner.precision()))
    }

    #[pyo3(signature = (value, n=0))]
    fn next_ask_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self.inner.next_ask_price(value, n, self.inner.precision()))
    }
}

/// Registers a named tick scheme for the lifetime of the process.
///
/// Names are matched without regard to ASCII case. A name cannot be replaced once registered.
///
/// # Errors
///
/// Returns an error if `name` is invalid or already registered.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3::pyfunction(name = "register_tick_scheme")]
pub fn py_register_tick_scheme(tick_scheme: &Bound<'_, PyAny>) -> PyResult<()> {
    if let Ok(scheme) = tick_scheme.extract::<PyRef<'_, PyFixedTickScheme>>() {
        return register_tick_scheme(&scheme.name, TickScheme::Fixed(scheme.inner))
            .map_err(to_pyvalue_err);
    }

    if let Ok(scheme) = tick_scheme.extract::<PyRef<'_, PyTieredTickScheme>>() {
        return register_tick_scheme(&scheme.name, TickScheme::Tiered(scheme.inner.clone()))
            .map_err(to_pyvalue_err);
    }
    Err(to_pytype_err(
        "tick_scheme must be a FixedTickScheme or TieredTickScheme",
    ))
}

/// Returns a registered tick scheme by name.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3::pyfunction(name = "get_tick_scheme")]
pub fn py_get_tick_scheme(py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
    let scheme = get_tick_scheme(name)
        .ok_or_else(|| to_pyvalue_err(format!("tick scheme {name} is not registered")))?;
    match scheme {
        TickScheme::Fixed(inner) => Ok(Py::new(
            py,
            PyFixedTickScheme::from_registered(name.to_string(), *inner),
        )?
        .into_any()),
        TickScheme::Tiered(inner) => Ok(Py::new(
            py,
            PyTieredTickScheme::from_registered(name.to_string(), inner.clone()),
        )?
        .into_any()),
        TickScheme::Betfair => Ok(Py::new(
            py,
            PyTieredTickScheme::from_registered(name.to_string(), BETFAIR_TICK_SCHEME.clone()),
        )?
        .into_any()),
        TickScheme::Crypto => Ok(Py::new(
            py,
            PyFixedTickScheme::from_registered(
                CRYPTO_0_01_TICK_SCHEME_NAME.to_string(),
                FixedTickScheme::new_with_precision(0.01, 2).map_err(to_pyvalue_err)?,
            ),
        )?
        .into_any()),
    }
}

/// Returns all registered tick scheme names in sorted order.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3::pyfunction(name = "list_tick_schemes")]
#[must_use]
pub fn py_list_tick_schemes() -> Vec<String> {
    list_tick_schemes()
}

fn check_tick_offset(n: i32) -> PyResult<()> {
    if n < 0 {
        return Err(to_pyvalue_err(format!("n must be >= 0, was {n}")));
    }
    Ok(())
}
