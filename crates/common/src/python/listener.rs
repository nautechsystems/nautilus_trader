#![cfg(feature = "live")]

use bytes::Bytes;
use futures::pin_mut;
use nautilus_core::python::{IntoPyObjectNautilusExt, call_python, to_pyruntime_err};
use pyo3::prelude::*;
use ustr::Ustr;

use crate::live::listener::MessageBusListener;

#[pymethods]
impl MessageBusListener {
    #[new]
    fn py_new() -> PyResult<Self> {
        Ok(Self::new())
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        !self.is_closed()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
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

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream_rx);
            while let Some(msg) = stream_rx.recv().await {
                Python::attach(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }
}
