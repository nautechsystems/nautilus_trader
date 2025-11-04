use pyo3::prelude::*;

use crate::common::{AsterdexMarketType, AsterdexOrderSide, AsterdexOrderType};

#[pyclass(name = "AsterdexMarketType")]
#[derive(Clone)]
pub struct PyAsterdexMarketType {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyAsterdexMarketType {
    #[staticmethod]
    fn spot() -> Self {
        Self {
            value: AsterdexMarketType::Spot.to_string(),
        }
    }

    #[staticmethod]
    fn futures() -> Self {
        Self {
            value: AsterdexMarketType::Futures.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("AsterdexMarketType.{}", self.value.to_uppercase())
    }
}

#[pyclass(name = "AsterdexOrderSide")]
#[derive(Clone)]
pub struct PyAsterdexOrderSide {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyAsterdexOrderSide {
    #[staticmethod]
    fn buy() -> Self {
        Self {
            value: AsterdexOrderSide::Buy.to_string(),
        }
    }

    #[staticmethod]
    fn sell() -> Self {
        Self {
            value: AsterdexOrderSide::Sell.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("AsterdexOrderSide.{}", self.value)
    }
}

#[pyclass(name = "AsterdexOrderType")]
#[derive(Clone)]
pub struct PyAsterdexOrderType {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl PyAsterdexOrderType {
    #[staticmethod]
    fn limit() -> Self {
        Self {
            value: AsterdexOrderType::Limit.to_string(),
        }
    }

    #[staticmethod]
    fn market() -> Self {
        Self {
            value: AsterdexOrderType::Market.to_string(),
        }
    }

    #[staticmethod]
    fn stop() -> Self {
        Self {
            value: AsterdexOrderType::Stop.to_string(),
        }
    }

    #[staticmethod]
    fn stop_market() -> Self {
        Self {
            value: AsterdexOrderType::StopMarket.to_string(),
        }
    }

    fn __repr__(&self) -> String {
        format!("AsterdexOrderType.{}", self.value)
    }
}
