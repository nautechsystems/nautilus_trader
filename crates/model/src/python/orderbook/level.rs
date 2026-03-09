use pyo3::prelude::*;

use crate::{
    data::order::BookOrder,
    orderbook::BookLevel,
    types::{price::Price, quantity::QuantityRaw},
};

#[pymethods]
impl BookLevel {
    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        // TODO: Return debug string for now
        format!("{self:?}")
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price.value
    }

    #[pyo3(name = "len")]
    fn py_len(&self) -> usize {
        self.len()
    }

    #[pyo3(name = "is_empty")]
    fn py_is_empty(&self) -> bool {
        self.is_empty()
    }

    #[pyo3(name = "size")]
    fn py_size(&self) -> f64 {
        self.size()
    }

    #[pyo3(name = "size_raw")]
    fn py_size_raw(&self) -> QuantityRaw {
        self.size_raw()
    }

    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> f64 {
        self.exposure()
    }

    #[pyo3(name = "exposure_raw")]
    fn py_exposure_raw(&self) -> QuantityRaw {
        self.exposure_raw()
    }

    #[pyo3(name = "first")]
    fn py_fist(&self) -> Option<BookOrder> {
        self.first().copied()
    }

    #[pyo3(name = "get_orders")]
    fn py_get_orders(&self) -> Vec<BookOrder> {
        self.get_orders()
    }
}
