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

use std::{collections::HashMap, sync::Arc};

use futures_util::{pin_mut, Stream, StreamExt};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{identifiers::InstrumentId, python::data::data_to_pycapsule};
use pyo3::prelude::*;

use crate::tardis::machine::{
    client::{determine_instrument_info, TardisMachineClient},
    message::WsMessage,
    parse::parse_tardis_ws_message,
    replay_normalized, stream_normalized, Error, InstrumentMiniInfo,
    ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions,
};

#[pymethods]
impl TardisMachineClient {
    #[new]
    fn py_new(base_url: &str) -> PyResult<Self> {
        Ok(Self::new(base_url))
    }

    #[pyo3(name = "is_closed")]
    pub fn py_is_closed(&self) -> bool {
        self.is_closed()
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
        let map = self.instruments.clone();

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            let stream = replay_normalized(&base_url, options, replay_signal)
                .await
                .expect("Failed to connect to WebSocket");

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            handle_python_stream(Box::pin(stream), callback, None, Some(map)).await;
            Ok(())
        })
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &self,
        instrument: InstrumentMiniInfo,
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
            handle_python_stream(Box::pin(stream), callback, Some(Arc::new(instrument)), None)
                .await;
            Ok(())
        })
    }
}

async fn handle_python_stream<S>(
    mut stream: S,
    callback: PyObject,
    instrument: Option<Arc<InstrumentMiniInfo>>,
    instrument_map: Option<HashMap<InstrumentId, Arc<InstrumentMiniInfo>>>,
) where
    S: Stream<Item = Result<WsMessage, Error>> + Unpin,
{
    pin_mut!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                // TODO: This sequence needs optimizing
                let info = if let Some(ref instrument) = instrument {
                    Some(instrument.clone())
                } else {
                    instrument_map
                        .as_ref()
                        .and_then(|map| determine_instrument_info(&msg, map))
                };

                if let Some(info) = info {
                    if let Some(data) = parse_tardis_ws_message(msg, info) {
                        Python::with_gil(|py| {
                            let py_obj = data_to_pycapsule(py, data);
                            call_python(py, &callback, py_obj);
                        });
                    } else {
                        continue; // Non-data message
                    }
                } else {
                    continue; // No instrument info
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
