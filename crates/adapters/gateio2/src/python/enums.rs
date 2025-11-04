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

//! Python wrappers for Gate.io enums.

use pyo3::prelude::*;

use crate::common::enums::{
    GateioAccountType, GateioMarketType, GateioOrderSide, GateioOrderStatus, GateioOrderType,
    GateioTimeInForce,
};

#[pyclass(name = "GateioMarketType")]
#[derive(Clone, Debug)]
pub struct PyGateioMarketType {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioMarketType {
    #[staticmethod]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self {
            value: GateioMarketType::Spot.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "MARGIN")]
    fn py_margin() -> Self {
        Self {
            value: GateioMarketType::Margin.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "FUTURES")]
    fn py_futures() -> Self {
        Self {
            value: GateioMarketType::Futures.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "DELIVERY")]
    fn py_delivery() -> Self {
        Self {
            value: GateioMarketType::Delivery.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "OPTIONS")]
    fn py_options() -> Self {
        Self {
            value: GateioMarketType::Options.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioMarketType.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "GateioOrderSide")]
#[derive(Clone, Debug)]
pub struct PyGateioOrderSide {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioOrderSide {
    #[staticmethod]
    #[pyo3(name = "BUY")]
    fn py_buy() -> Self {
        Self {
            value: GateioOrderSide::Buy.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "SELL")]
    fn py_sell() -> Self {
        Self {
            value: GateioOrderSide::Sell.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioOrderSide.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "GateioOrderType")]
#[derive(Clone, Debug)]
pub struct PyGateioOrderType {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioOrderType {
    #[staticmethod]
    #[pyo3(name = "LIMIT")]
    fn py_limit() -> Self {
        Self {
            value: GateioOrderType::Limit.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "MARKET")]
    fn py_market() -> Self {
        Self {
            value: GateioOrderType::Market.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioOrderType.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "GateioTimeInForce")]
#[derive(Clone, Debug)]
pub struct PyGateioTimeInForce {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioTimeInForce {
    #[staticmethod]
    #[pyo3(name = "GTC")]
    fn py_gtc() -> Self {
        Self {
            value: GateioTimeInForce::GTC.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "IOC")]
    fn py_ioc() -> Self {
        Self {
            value: GateioTimeInForce::IOC.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "POC")]
    fn py_poc() -> Self {
        Self {
            value: GateioTimeInForce::POC.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "FOK")]
    fn py_fok() -> Self {
        Self {
            value: GateioTimeInForce::FOK.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioTimeInForce.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "GateioOrderStatus")]
#[derive(Clone, Debug)]
pub struct PyGateioOrderStatus {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioOrderStatus {
    #[staticmethod]
    #[pyo3(name = "OPEN")]
    fn py_open() -> Self {
        Self {
            value: GateioOrderStatus::Open.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "CLOSED")]
    fn py_closed() -> Self {
        Self {
            value: GateioOrderStatus::Closed.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "CANCELLED")]
    fn py_cancelled() -> Self {
        Self {
            value: GateioOrderStatus::Cancelled.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioOrderStatus.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "GateioAccountType")]
#[derive(Clone, Debug)]
pub struct PyGateioAccountType {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyGateioAccountType {
    #[staticmethod]
    #[pyo3(name = "SPOT")]
    fn py_spot() -> Self {
        Self {
            value: GateioAccountType::Spot.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "MARGIN")]
    fn py_margin() -> Self {
        Self {
            value: GateioAccountType::Margin.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "FUTURES")]
    fn py_futures() -> Self {
        Self {
            value: GateioAccountType::Futures.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "DELIVERY")]
    fn py_delivery() -> Self {
        Self {
            value: GateioAccountType::Delivery.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "OPTIONS")]
    fn py_options() -> Self {
        Self {
            value: GateioAccountType::Options.to_string(),
        }
    }

    #[staticmethod]
    #[pyo3(name = "UNIFIED")]
    fn py_unified() -> Self {
        Self {
            value: GateioAccountType::Unified.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("GateioAccountType.{}", self.value.to_uppercase())
    }
}
