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

use std::{fs, i128, str::FromStr};

use databento::{dbn, live::Subscription};
use indexmap::IndexMap;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    identifiers::Venue,
    python::{data::data_to_pycapsule, instruments::instrument_any_to_pyobject},
};
use pyo3::prelude::*;
use time::OffsetDateTime;

use crate::databento::{
    live::{DatabentoFeedHandler, LiveCommand, LiveMessage},
    symbology::{check_consistent_symbology, infer_symbology_type},
    types::DatabentoPublisher,
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoLiveClient {
    #[pyo3(get)]
    pub key: String,
    #[pyo3(get)]
    pub dataset: String,
    #[pyo3(get)]
    pub is_running: bool,
    #[pyo3(get)]
    pub is_closed: bool,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<LiveCommand>,
    cmd_rx: Option<tokio::sync::mpsc::UnboundedReceiver<LiveCommand>>,
    buffer_size: usize,
    publisher_venue_map: IndexMap<u16, Venue>,
}

impl DatabentoLiveClient {
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.cmd_tx.is_closed()
    }

    async fn process_messages(
        mut msg_rx: tokio::sync::mpsc::Receiver<LiveMessage>,
        callback: PyObject,
        callback_pyo3: PyObject,
    ) -> PyResult<()> {
        tracing::debug!("Processing messages...");
        // Continue to process messages until channel is hung up
        while let Some(msg) = msg_rx.recv().await {
            tracing::trace!("Received message: {:?}", msg);
            let result = match msg {
                LiveMessage::Data(data) => Python::with_gil(|py| {
                    let py_obj = data_to_pycapsule(py, data);
                    call_python(py, &callback, py_obj)
                }),
                LiveMessage::Instrument(data) => Python::with_gil(|py| {
                    let py_obj =
                        instrument_any_to_pyobject(py, data).expect("Error creating instrument");
                    call_python(py, &callback, py_obj)
                }),
                LiveMessage::Status(data) => Python::with_gil(|py| {
                    let py_obj = data.into_py(py);
                    call_python(py, &callback_pyo3, py_obj)
                }),
                LiveMessage::Imbalance(data) => Python::with_gil(|py| {
                    let py_obj = data.into_py(py);
                    call_python(py, &callback_pyo3, py_obj)
                }),
                LiveMessage::Statistics(data) => Python::with_gil(|py| {
                    let py_obj = data.into_py(py);
                    call_python(py, &callback_pyo3, py_obj)
                }),
                LiveMessage::Close => {
                    // Graceful close
                    break;
                }
                LiveMessage::Error(e) => {
                    // Return error to Python
                    return Err(to_pyruntime_err(e));
                }
            };

            result?;
        }

        msg_rx.close();
        tracing::debug!("Closed message receiver");

        Ok(())
    }

    fn send_command(&self, cmd: LiveCommand) -> PyResult<()> {
        self.cmd_tx.send(cmd).map_err(to_pyruntime_err)
    }
}

fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) -> PyResult<()> {
    callback.call1(py, (py_obj,)).map_err(|e| {
        tracing::error!("Error calling Python: {e}");
        e
    })?;
    Ok(())
}

#[pymethods]
impl DatabentoLiveClient {
    #[new]
    pub fn py_new(key: String, dataset: String, publishers_path: String) -> PyResult<Self> {
        let publishers_json = fs::read_to_string(publishers_path)?;
        let publishers_vec: Vec<DatabentoPublisher> =
            serde_json::from_str(&publishers_json).map_err(to_pyvalue_err)?;
        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<LiveCommand>();

        // Hard coded to a reasonable size for now
        let buffer_size = 100_000;

        Ok(Self {
            key,
            dataset,
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            buffer_size,
            is_running: false,
            is_closed: false,
            publisher_venue_map,
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe(
        &mut self,
        schema: String,
        symbols: Vec<String>,
        start: Option<u64>,
        snapshot: Option<bool>,
    ) -> PyResult<()> {
        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(|s| s.as_str()).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let mut sub = Subscription::builder()
            .symbols(symbols)
            .schema(dbn::Schema::from_str(&schema).map_err(to_pyvalue_err)?)
            .stype_in(dbn::SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
            .build();

        if let Some(start) = start {
            let start = OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))
                .map_err(to_pyvalue_err)?;
            sub.start = Some(start);
        };
        sub.use_snapshot = snapshot.unwrap_or(false);

        self.send_command(LiveCommand::Subscribe(sub))
    }

    #[pyo3(name = "start")]
    fn py_start<'py>(
        &mut self,
        py: Python<'py>,
        callback: PyObject,
        callback_pyo3: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.is_closed {
            return Err(to_pyruntime_err("Client already closed"));
        };
        if self.is_running {
            return Err(to_pyruntime_err("Client already running"));
        };

        tracing::debug!("Starting client");

        self.is_running = true;

        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<LiveMessage>(self.buffer_size);

        // Consume the receiver
        // SAFETY: We guard the client from being started more than once with the
        // `is_running` flag, so here it is safe to unwrap the command receiver.
        let cmd_rx = self.cmd_rx.take().unwrap();

        let mut feed_handler = DatabentoFeedHandler::new(
            self.key.clone(),
            self.dataset.clone(),
            cmd_rx,
            msg_tx,
            self.publisher_venue_map.clone(),
        );

        self.send_command(LiveCommand::Start)?;

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            let (proc_handle, feed_handle) = tokio::join!(
                Self::process_messages(msg_rx, callback, callback_pyo3),
                feed_handler.run(),
            );

            match proc_handle {
                Ok(()) => tracing::debug!("Message processor completed"),
                Err(e) => tracing::error!("Message processor error: {e}"),
            }

            match feed_handle {
                Ok(()) => tracing::debug!("Feed handler completed"),
                Err(e) => tracing::error!("Feed handler error: {e}"),
            }

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) -> PyResult<()> {
        if !self.is_running {
            return Err(to_pyruntime_err("Client never started"));
        };
        if self.is_closed {
            return Err(to_pyruntime_err("Client already closed"));
        };

        tracing::debug!("Closing client");

        if !self.is_closed() {
            self.send_command(LiveCommand::Close)?;
        }

        self.is_running = false;
        self.is_closed = true;

        Ok(())
    }
}
