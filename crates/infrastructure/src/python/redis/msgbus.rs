use bytes::Bytes;
use futures::{pin_mut, stream::StreamExt};
use nautilus_common::msgbus::database::MessageBusDatabaseAdapter;
use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, call_python, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::identifiers::TraderId;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::redis::msgbus::RedisMessageBusDatabase;

#[pymethods]
impl RedisMessageBusDatabase {
    #[new]
    fn py_new(trader_id: TraderId, instance_id: UUID4, config_json: Vec<u8>) -> PyResult<Self> {
        let config = serde_json::from_slice(&config_json).map_err(to_pyvalue_err)?;
        Self::new(trader_id, instance_id, config).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "publish")]
    fn py_publish(&self, topic: String, payload: Vec<u8>) {
        self.publish(Ustr::from(&topic), Bytes::from(payload));
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: Py<PyAny>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.get_stream_receiver().map_err(to_pyruntime_err)?;
        let stream = Self::stream(stream_rx);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream);
            while let Some(msg) = stream.next().await {
                Python::attach(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }
}
