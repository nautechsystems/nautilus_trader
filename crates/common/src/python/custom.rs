use bytes::Bytes;
use nautilus_core::UnixNanos;
use nautilus_model::data::DataType;
use pyo3::prelude::*;

use crate::custom::CustomData;

#[pymethods]
impl CustomData {
    #[new]
    fn py_new(data_type: DataType, value: Vec<u8>, ts_event: u64, ts_init: u64) -> Self {
        Self::new(
            data_type,
            Bytes::from(value),
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    #[getter]
    #[pyo3(name = "data_type")]
    fn py_data_type(&self) -> DataType {
        self.data_type.clone()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> Vec<u8> {
        self.value.to_vec()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    const fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}
