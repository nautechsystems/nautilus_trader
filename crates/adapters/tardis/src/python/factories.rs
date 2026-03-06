use pyo3::prelude::*;

use crate::{common::consts::TARDIS, factories::TardisDataClientFactory};

#[pymethods]
impl TardisDataClientFactory {
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        TARDIS
    }
}
