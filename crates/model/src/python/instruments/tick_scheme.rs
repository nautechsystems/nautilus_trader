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

//! Python access to named fixed and tiered tick schemes.

use nautilus_core::{
    correctness::check_in_range_inclusive_u8,
    python::{to_pytype_err, to_pyvalue_err},
};
use pyo3::{prelude::*, types::PyAnyMethods};

use crate::{
    instruments::{
        FixedTickScheme, TickScheme, TickSchemeRule, TieredTickScheme, get_tick_scheme,
        list_tick_schemes, register_tick_scheme, tick_scheme::CRYPTO_0_01_TICK_SCHEME,
    },
    types::{Price, fixed::FIXED_PRECISION, price::PriceRaw},
};

#[derive(Clone, Debug)]
#[pyo3::pyclass(
    frozen,
    name = "FixedTickScheme",
    module = "nautilus_trader.model",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
pub struct PyFixedTickScheme {
    name: String,
    scheme: FixedTickScheme,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFixedTickScheme {
    #[new]
    #[pyo3(signature = (name, price_precision, increment=None))]
    fn py_new(name: String, price_precision: u8, increment: Option<Price>) -> PyResult<Self> {
        check_in_range_inclusive_u8(price_precision, 0, FIXED_PRECISION, "precision")
            .map_err(to_pyvalue_err)?;

        let tick = increment.unwrap_or_else(|| {
            let raw = PriceRaw::pow(10, u32::from(FIXED_PRECISION - price_precision));
            Price::from_raw(raw, price_precision)
        });

        let scheme =
            FixedTickScheme::new_with_precision(tick, price_precision).map_err(to_pyvalue_err)?;
        Ok(Self { name, scheme })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.scheme.precision()
    }

    #[getter]
    fn increment(&self) -> Price {
        self.scheme.tick()
    }

    #[pyo3(signature = (value, n=0))]
    fn next_bid_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self
            .scheme
            .next_bid_price(value, n, self.scheme.precision()))
    }

    #[pyo3(signature = (value, n=0))]
    fn next_ask_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self
            .scheme
            .next_ask_price(value, n, self.scheme.precision()))
    }
}

#[derive(Clone, Debug)]
#[pyo3::pyclass(
    frozen,
    name = "TieredTickScheme",
    module = "nautilus_trader.model",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
pub struct PyTieredTickScheme {
    name: String,
    scheme: TieredTickScheme,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyTieredTickScheme {
    #[new]
    #[pyo3(signature = (name, tiers, price_precision, max_ticks_per_tier=100))]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts tier definitions into an owned vector"
    )]
    fn py_new(
        name: String,
        tiers: Vec<(f64, f64, f64)>,
        price_precision: u8,
        max_ticks_per_tier: usize,
    ) -> PyResult<Self> {
        let scheme = TieredTickScheme::new(&tiers, price_precision, max_ticks_per_tier)
            .map_err(to_pyvalue_err)?;
        Ok(Self { name, scheme })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn price_precision(&self) -> u8 {
        self.scheme.precision()
    }

    #[getter]
    fn min_price(&self) -> Price {
        self.scheme.min_price()
    }

    #[getter]
    fn max_price(&self) -> Price {
        self.scheme.max_price()
    }

    #[getter]
    fn ticks(&self) -> Vec<Price> {
        self.scheme.ticks()
    }

    #[getter]
    fn tick_count(&self) -> usize {
        self.scheme.tick_count()
    }

    #[pyo3(signature = (value, n=0))]
    fn next_bid_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self
            .scheme
            .next_bid_price(value, n, self.scheme.precision()))
    }

    #[pyo3(signature = (value, n=0))]
    fn next_ask_price(&self, value: f64, n: i32) -> PyResult<Option<Price>> {
        check_tick_offset(n)?;
        Ok(self
            .scheme
            .next_ask_price(value, n, self.scheme.precision()))
    }
}

/// Registers a named tick scheme for the lifetime of the process.
///
/// Names are trimmed and matched without regard to ASCII case. Registered names,
/// including built-in names and aliases, cannot be replaced or removed.
///
/// # Errors
///
/// Returns an error if the name is invalid or already registered.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyfunction(name = "register_tick_scheme")]
pub fn py_register_tick_scheme(
    #[gen_stub(override_type(type_repr = "FixedTickScheme | TieredTickScheme"))]
    tick_scheme: &Bound<'_, PyAny>,
) -> PyResult<()> {
    if let Ok(fixed) = tick_scheme.extract::<PyRef<'_, PyFixedTickScheme>>() {
        register_tick_scheme(&fixed.name, TickScheme::Fixed(fixed.scheme)).map_err(to_pyvalue_err)
    } else if let Ok(tiered) = tick_scheme.extract::<PyRef<'_, PyTieredTickScheme>>() {
        register_tick_scheme(&tiered.name, TickScheme::Tiered(tiered.scheme.clone()))
            .map_err(to_pyvalue_err)
    } else {
        Err(to_pytype_err(
            "tick_scheme must be a FixedTickScheme or TieredTickScheme",
        ))
    }
}

/// Returns a copy of a registered scheme, matching trimmed names without regard to ASCII case.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyfunction(name = "get_tick_scheme")]
#[gen_stub(override_return_type(type_repr = "FixedTickScheme | TieredTickScheme"))]
pub fn py_get_tick_scheme(py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
    let scheme = get_tick_scheme(name)
        .ok_or_else(|| to_pyvalue_err(format!("unknown tick scheme {name}")))?;
    let name = name.trim().to_ascii_uppercase();

    match scheme {
        TickScheme::Fixed(scheme) => {
            Ok(Py::new(py, PyFixedTickScheme { name, scheme })?.into_any())
        }
        TickScheme::Tiered(scheme) => {
            Ok(Py::new(py, PyTieredTickScheme { name, scheme })?.into_any())
        }
        TickScheme::Betfair => Ok(Py::new(
            py,
            PyTieredTickScheme {
                name,
                scheme: TieredTickScheme::betfair(),
            },
        )?
        .into_any()),
        TickScheme::Crypto => Ok(Py::new(
            py,
            PyFixedTickScheme {
                name,
                scheme: *CRYPTO_0_01_TICK_SCHEME,
            },
        )?
        .into_any()),
    }
}

/// Returns all registered names in uppercase, sorted lexicographically.
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyfunction(name = "list_tick_schemes")]
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
