// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, path::Path, sync::Arc};

use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err};
use nautilus_model::{
    data::{Bar, Data},
    python::data::data_to_pycapsule,
};
use pyo3::{prelude::*, types::PyList};

use crate::{
    machine::{
        Error,
        client::{TardisMachineClient, determine_instrument_info},
        message::WsMessage,
        parse::parse_tardis_ws_message,
        replay_normalized, stream_normalized,
        types::{
            InstrumentMiniInfo, ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions,
            TardisInstrumentKey,
        },
    },
    replay::run_tardis_machine_replay_from_config,
};

#[pymethods]
impl ReplayNormalizedRequestOptions {
    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: Vec<u8>) -> Self {
        serde_json::from_slice(&data).expect("Failed to parse JSON")
    }

    #[pyo3(name = "from_json_array")]
    #[staticmethod]
    fn py_from_json_array(data: Vec<u8>) -> Vec<Self> {
        serde_json::from_slice(&data).expect("Failed to parse JSON array")
    }
}

#[pymethods]
impl StreamNormalizedRequestOptions {
    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: Vec<u8>) -> Self {
        serde_json::from_slice(&data).expect("Failed to parse JSON")
    }

    #[pyo3(name = "from_json_array")]
    #[staticmethod]
    fn py_from_json_array(data: Vec<u8>) -> Vec<Self> {
        serde_json::from_slice(&data).expect("Failed to parse JSON array")
    }
}

#[pymethods]
impl TardisMachineClient {
    #[new]
    #[pyo3(signature = (base_url=None, normalize_symbols=true))]
    fn py_new(base_url: Option<&str>, normalize_symbols: bool) -> PyResult<Self> {
        Self::new(base_url, normalize_symbols).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    pub fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }

    #[pyo3(name = "replay")]
    fn py_replay<'py>(
        &self,
        instruments: Vec<InstrumentMiniInfo>,
        options: Vec<ReplayNormalizedRequestOptions>,
        callback: PyObject,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let map = if instruments.is_empty() {
            self.instruments.clone()
        } else {
            let mut instrument_map: HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>> =
                HashMap::new();
            for inst in instruments {
                let key = inst.as_tardis_instrument_key();
                instrument_map.insert(key, Arc::new(inst.clone()));
            }
            instrument_map
        };

        let base_url = self.base_url.clone();
        let replay_signal = self.replay_signal.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let stream = replay_normalized(&base_url, options, replay_signal)
                .await
                .map_err(to_pyruntime_err)?;

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            handle_python_stream(Box::pin(stream), callback, None, Some(map)).await;
            Ok(())
        })
    }

    #[pyo3(name = "replay_bars")]
    fn py_replay_bars<'py>(
        &self,
        instruments: Vec<InstrumentMiniInfo>,
        options: Vec<ReplayNormalizedRequestOptions>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let map = if instruments.is_empty() {
            self.instruments.clone()
        } else {
            instruments
                .into_iter()
                .map(|inst| (inst.as_tardis_instrument_key(), Arc::new(inst)))
                .collect()
        };

        let base_url = self.base_url.clone();
        let replay_signal = self.replay_signal.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let stream = replay_normalized(&base_url, options, replay_signal)
                .await
                .map_err(to_pyruntime_err)?;

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            pin_mut!(stream);

            let mut bars: Vec<Bar> = Vec::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(msg) => {
                        if let Some(Data::Bar(bar)) = determine_instrument_info(&msg, &map)
                            .and_then(|info| parse_tardis_ws_message(msg, info))
                        {
                            bars.push(bar);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error in WebSocket stream: {e:?}");
                        break;
                    }
                }
            }

            Python::with_gil(|py| {
                let pylist =
                    PyList::new(py, bars.into_iter().map(|bar| bar.into_py_any_unwrap(py)))
                        .expect("Invalid `ExactSizeIterator`");
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &self,
        instruments: Vec<InstrumentMiniInfo>,
        options: Vec<StreamNormalizedRequestOptions>,
        callback: PyObject,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut instrument_map: HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>> =
            HashMap::new();
        for inst in instruments {
            let key = inst.as_tardis_instrument_key();
            instrument_map.insert(key, Arc::new(inst.clone()));
        }

        let base_url = self.base_url.clone();
        let replay_signal = self.replay_signal.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let stream = stream_normalized(&base_url, options, replay_signal)
                .await
                .expect("Failed to connect to WebSocket");

            // We use Box::pin to heap-allocate the stream and ensure it implements
            // Unpin for safe async handling across lifetimes.
            handle_python_stream(Box::pin(stream), callback, None, Some(instrument_map)).await;
            Ok(())
        })
    }
}

#[pyfunction]
#[pyo3(name = "run_tardis_machine_replay")]
#[pyo3(signature = (config_filepath))]
pub fn py_run_tardis_machine_replay(
    py: Python<'_>,
    config_filepath: String,
) -> PyResult<Bound<'_, PyAny>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let config_filepath = Path::new(&config_filepath);
        run_tardis_machine_replay_from_config(config_filepath)
            .await
            .map_err(to_pyruntime_err)?;
        Ok(())
    })
}

async fn handle_python_stream<S>(
    stream: S,
    callback: PyObject,
    instrument: Option<Arc<InstrumentMiniInfo>>,
    instrument_map: Option<HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>>>,
) where
    S: Stream<Item = Result<WsMessage, Error>> + Unpin,
{
    pin_mut!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                let info = instrument.clone().or_else(|| {
                    instrument_map
                        .as_ref()
                        .and_then(|map| determine_instrument_info(&msg, map))
                });

                if let Some(info) = info {
                    if let Some(data) = parse_tardis_ws_message(msg, info) {
                        Python::with_gil(|py| {
                            let py_obj = data_to_pycapsule(py, data);
                            call_python(py, &callback, py_obj);
                        });
                    }
                }
            }
            Err(e) => {
                tracing::error!("Error in WebSocket stream: {e:?}");
                break;
            }
        }
    }
}

fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}
