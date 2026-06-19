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

//! Python bindings for the Databento live client.

use std::path::PathBuf;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    identifiers::InstrumentId,
    python::{data::data_to_pycapsule, instruments::instrument_any_to_pyobject},
};
use pyo3::prelude::*;

use super::types::DatabentoSubscriptionAck;
pub use crate::live::DatabentoLiveClient;
use crate::live::{DatabentoMessage, is_command_send_error};

impl DatabentoLiveClient {
    async fn process_messages(
        mut msg_rx: tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
        callback: Py<PyAny>,
        callback_pyo3: Py<PyAny>,
    ) -> PyResult<()> {
        log::debug!("Processing messages...");
        // Continue to process messages until channel is hung up
        while let Some(msg) = msg_rx.recv().await {
            log::trace!("Received message: {msg:?}");

            match msg {
                DatabentoMessage::Data(data) => Python::attach(|py| {
                    let py_obj = data_to_pycapsule(py, data);
                    call_python(py, &callback, py_obj);
                }),
                DatabentoMessage::Instrument(data) => {
                    Python::attach(|py| match instrument_any_to_pyobject(py, *data) {
                        Ok(py_obj) => call_python(py, &callback, py_obj),
                        Err(e) => log::error!("Failed creating instrument: {e}"),
                    });
                }
                DatabentoMessage::Status(data) => Python::attach(|py| {
                    let py_obj = data.into_py_any_unwrap(py);
                    call_python(py, &callback_pyo3, py_obj);
                }),
                DatabentoMessage::Imbalance(data) => Python::attach(|py| {
                    let py_obj = data.into_py_any_unwrap(py);
                    call_python(py, &callback_pyo3, py_obj);
                }),
                DatabentoMessage::Statistics(data) => Python::attach(|py| {
                    let py_obj = data.into_py_any_unwrap(py);
                    call_python(py, &callback_pyo3, py_obj);
                }),
                DatabentoMessage::SubscriptionAck(ack) => Python::attach(|py| {
                    let py_obj: DatabentoSubscriptionAck = ack.into();
                    let py_obj = py_obj.into_py_any_unwrap(py);
                    call_python(py, &callback_pyo3, py_obj);
                }),
                DatabentoMessage::Close => {
                    // Graceful close
                    break;
                }
                DatabentoMessage::Error(e) => {
                    // Return error to Python
                    return Err(to_pyruntime_err(e));
                }
            }
        }

        msg_rx.close();
        log::debug!("Closed message receiver");

        Ok(())
    }
}

fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        // TODO: Improve this by checking for the actual exception type
        if !e.to_string().contains("CancelledError") {
            log::error!("Error calling Python: {e}");
        }
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DatabentoLiveClient {
    /// Creates a new `DatabentoLiveClient` instance.
    #[new]
    #[pyo3(signature = (key, dataset, publishers_filepath, use_exchange_as_venue, bars_timestamp_on_close=None, reconnect_timeout_mins=None))]
    pub fn py_new(
        key: String,
        dataset: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: Option<bool>,
        reconnect_timeout_mins: Option<i64>,
    ) -> PyResult<Self> {
        Self::new(
            key,
            dataset,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
            reconnect_timeout_mins,
        )
        .map_err(to_pyvalue_err)
    }

    #[getter]
    fn dataset(&self) -> &str {
        self.dataset.as_str()
    }

    #[pyo3(name = "is_running")]
    const fn py_is_running(&self) -> bool {
        self.is_running()
    }

    #[pyo3(name = "is_closed")]
    const fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Subscribes to Databento live data for the requested instruments.
    #[pyo3(name = "subscribe")]
    #[pyo3(signature = (schema, instrument_ids, start=None, snapshot=None, price_precisions=None, stype_in=None))]
    fn py_subscribe(
        &mut self,
        schema: String,
        instrument_ids: Vec<InstrumentId>,
        start: Option<u64>,
        snapshot: Option<bool>,
        price_precisions: Option<Vec<Option<u8>>>,
        stype_in: Option<String>,
    ) -> PyResult<()> {
        if let Err(e) = self.subscribe(
            schema,
            instrument_ids,
            start,
            snapshot,
            price_precisions,
            stype_in,
        ) {
            return if is_command_send_error(&e) {
                Err(to_pyruntime_err(e))
            } else {
                Err(to_pyvalue_err(e))
            };
        }

        Ok(())
    }

    /// Starts the live feed handler and returns its message receiver.
    #[pyo3(name = "start")]
    fn py_start<'py>(
        &mut self,
        py: Python<'py>,
        callback: Py<PyAny>,
        callback_pyo3: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (mut feed_handler, msg_rx) = self.start().map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (proc_handle, feed_handle) = tokio::join!(
                Self::process_messages(msg_rx, callback, callback_pyo3),
                feed_handler.run(),
            );

            if let Err(e) = proc_handle {
                log::error!("Message processor error: {e}");
                return Err(e);
            }

            if let Err(e) = feed_handle {
                log::error!("Feed handler error: {e}");
                return Err(to_pyruntime_err(e));
            }

            log::debug!("Live client completed");
            Ok(())
        })
    }

    /// Closes the live client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client was never started, is already closed, or cannot send
    /// the close command to the feed handler.
    #[pyo3(name = "close")]
    fn py_close(&mut self) -> PyResult<()> {
        self.close().map_err(to_pyruntime_err)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pyo3::exceptions::{PyRuntimeError, PyValueError};
    use rstest::rstest;

    use super::*;

    fn create_test_client() -> DatabentoLiveClient {
        DatabentoLiveClient::new(
            "test-api-key".to_string(),
            "GLBX.MDP3".to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            None,
            None,
        )
        .unwrap()
    }

    #[rstest]
    fn test_py_subscribe_maps_invalid_input_to_value_error() {
        Python::initialize();
        let mut client = create_test_client();

        let err = client
            .py_subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("not-a-stype".to_string()),
            )
            .unwrap_err();

        Python::attach(|py| {
            assert!(err.is_instance_of::<PyValueError>(py));
        });
    }

    #[rstest]
    fn test_py_subscribe_maps_command_send_error_to_runtime_error() {
        Python::initialize();
        let mut client = create_test_client();
        let (feed_handler, msg_rx) = client.start().unwrap();
        drop(feed_handler);
        drop(msg_rx);

        let err = client
            .py_subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("parent".to_string()),
            )
            .unwrap_err();

        Python::attach(|py| {
            assert!(err.is_instance_of::<PyRuntimeError>(py));
        });
    }
}
