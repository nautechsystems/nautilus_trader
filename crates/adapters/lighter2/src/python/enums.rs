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

//! Python bindings for Lighter enums.

use pyo3::prelude::*;

use crate::common::enums::*;

/// Python wrapper for `LighterAccountType`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterAccountType")]
#[derive(Clone, Copy)]
pub struct PyLighterAccountType(pub LighterAccountType);

#[pymethods]
impl PyLighterAccountType {
    #[new]
    fn new(value: &str) -> PyResult<Self> {
        match value.to_lowercase().as_str() {
            "standard" => Ok(Self(LighterAccountType::Standard)),
            "premium" => Ok(Self(LighterAccountType::Premium)),
            _ => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Invalid account type: {}", value),
            )),
        }
    }

    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterAccountType('{}')", self.0)
    }
}

/// Python wrapper for `LighterOrderType`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterOrderType")]
#[derive(Clone, Copy)]
pub struct PyLighterOrderType(pub LighterOrderType);

#[pymethods]
impl PyLighterOrderType {
    #[staticmethod]
    fn limit() -> Self {
        Self(LighterOrderType::Limit)
    }

    #[staticmethod]
    fn market() -> Self {
        Self(LighterOrderType::Market)
    }

    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterOrderType('{}')", self.0)
    }
}

/// Python wrapper for `LighterTimeInForce`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterTimeInForce")]
#[derive(Clone, Copy)]
pub struct PyLighterTimeInForce(pub LighterTimeInForce);

#[pymethods]
impl PyLighterTimeInForce {
    #[staticmethod]
    fn gtc() -> Self {
        Self(LighterTimeInForce::GoodTilCanceled)
    }

    #[staticmethod]
    fn ioc() -> Self {
        Self(LighterTimeInForce::ImmediateOrCancel)
    }

    #[staticmethod]
    fn fok() -> Self {
        Self(LighterTimeInForce::FillOrKill)
    }

    #[staticmethod]
    fn post_only() -> Self {
        Self(LighterTimeInForce::PostOnly)
    }

    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterTimeInForce('{}')", self.0)
    }
}

/// Python wrapper for `LighterOrderSide`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterOrderSide")]
#[derive(Clone, Copy)]
pub struct PyLighterOrderSide(pub LighterOrderSide);

#[pymethods]
impl PyLighterOrderSide {
    #[staticmethod]
    fn buy() -> Self {
        Self(LighterOrderSide::Buy)
    }

    #[staticmethod]
    fn sell() -> Self {
        Self(LighterOrderSide::Sell)
    }

    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterOrderSide('{}')", self.0)
    }
}

/// Python wrapper for `LighterOrderStatus`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterOrderStatus")]
#[derive(Clone, Copy)]
pub struct PyLighterOrderStatus(pub LighterOrderStatus);

#[pymethods]
impl PyLighterOrderStatus {
    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterOrderStatus('{}')", self.0)
    }
}

/// Python wrapper for `LighterInstrumentType`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterInstrumentType")]
#[derive(Clone, Copy)]
pub struct PyLighterInstrumentType(pub LighterInstrumentType);

#[pymethods]
impl PyLighterInstrumentType {
    #[staticmethod]
    fn spot() -> Self {
        Self(LighterInstrumentType::Spot)
    }

    #[staticmethod]
    fn perp() -> Self {
        Self(LighterInstrumentType::Perp)
    }

    fn __str__(&self) -> String {
        format!("{}", self.0)
    }

    fn __repr__(&self) -> String {
        format!("LighterInstrumentType('{}')", self.0)
    }
}
