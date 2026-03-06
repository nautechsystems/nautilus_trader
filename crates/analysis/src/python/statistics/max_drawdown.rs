use std::collections::BTreeMap;

use pyo3::prelude::*;

use super::transform_returns;
use crate::{statistic::PortfolioStatistic, statistics::max_drawdown::MaxDrawdown};

#[pymethods]
impl MaxDrawdown {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "calculate_from_returns")]
    fn py_calculate_from_returns(&self, raw_returns: BTreeMap<u64, f64>) -> Option<f64> {
        self.calculate_from_returns(&transform_returns(raw_returns))
    }

    fn __repr__(&self) -> String {
        format!("MaxDrawdown({})", self.name())
    }
}
