// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use futures_util::{pin_mut, Stream, StreamExt};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::python::data::data_to_pycapsule;
use pyo3::prelude::*;

use crate::tardis::machine::{
    client::TardisClient, enums::WsMessage, parse::parse_tardis_ws_message, replay_normalized,
    stream_normalized, Error, ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions,
};

#[pymethods]
impl TardisClient {
    #[new]
    fn py_new(base_url: &str) -> PyResult<Self> {
        Ok(Self::new(base_url))
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close()
    }

    #[pyo3(name = "replay")]
    fn py_replay<'py>(
        &self,
        options: Vec<ReplayNormalizedRequestOptions>,
        callback: PyObject,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let base_url = self.base_url.clone();
        let replay_signal = self.replay_signal.clone();

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            let stream = replay_normalized(&base_url, options, replay_signal)
                .await
                .expect("Failed to connect to WebSocket");

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            handle_python_stream(Box::pin(stream), callback).await;
            Ok(())
        })
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &self,
        options: Vec<StreamNormalizedRequestOptions>,
        callback: PyObject,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let base_url = self.base_url.clone();
        let replay_signal = self.replay_signal.clone();

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            let stream = stream_normalized(&base_url, options, replay_signal)
                .await
                .expect("Failed to connect to WebSocket");

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            handle_python_stream(Box::pin(stream), callback).await;
            Ok(())
        })
    }
}

async fn handle_python_stream<S>(mut stream: S, callback: PyObject)
where
    S: Stream<Item = Result<WsMessage, Error>> + Unpin,
{
    pin_mut!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                if let Some(data) = parse_tardis_ws_message(msg, 0, 0) {
                    Python::with_gil(|py| {
                        let py_obj = data_to_pycapsule(py, data);
                        call_python(py, &callback, py_obj);
                    });
                } else {
                    continue;
                }
            }
            Err(e) => {
                tracing::error!("Error in WebSocket stream: {e:?}");
                break;
            }
        }
    }
}

pub fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) -> PyResult<()> {
    callback.call1(py, (py_obj,)).map_err(|e| {
        tracing::error!("Error calling Python: {e}");
        e
    })?;
    Ok(())
}
