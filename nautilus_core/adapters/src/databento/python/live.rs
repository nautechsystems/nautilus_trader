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

use std::{collections::HashMap, fs, str::FromStr, sync::mpsc::Sender};

use databento::live::Subscription;
use indexmap::IndexMap;
use nautilus_common::runtime::get_runtime;
use nautilus_core::time::UnixNanos;
use nautilus_model::{
    data::Data,
    identifiers::{symbol::Symbol, venue::Venue},
    instruments::Instrument,
    python::data::data_to_pycapsule,
};
use pyo3::prelude::*;
use time::OffsetDateTime;
use tokio::sync::mpsc::Receiver;

use super::loader::convert_instrument_to_pyobject;
use crate::databento::{
    live::{DatabentoFeedHandler, LiveCommand, LiveMessage, StartCommand},
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
    tx: Sender<LiveCommand>,
    rx: Option<Receiver<LiveMessage>>,
    glbx_exchange_map: HashMap<Symbol, Venue>,
}

impl DatabentoLiveClient {
    async fn process_messages(
        mut rx: Receiver<LiveMessage>,
        callback: PyObject,
    ) -> anyhow::Result<()> {
        while let Some(msg) = rx.recv().await {
            match msg {
                LiveMessage::Data(data) => {
                    Python::with_gil(|py| {
                        call_python_with_data(py, &callback, data);
                    });
                }
                LiveMessage::Instrument(inst) => {
                    Python::with_gil(|py| {
                        call_python_with_instrument(py, &callback, inst);
                    });
                }
                LiveMessage::Error(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn send_command(&self, cmd: LiveCommand) -> anyhow::Result<()> {
        self.tx.send(cmd).map_err(anyhow::Error::new)
    }
}

#[pymethods]
impl DatabentoLiveClient {
    #[new]
    pub fn py_new(key: String, dataset: String, publishers_path: String) -> anyhow::Result<Self> {
        let file_content = fs::read_to_string(publishers_path)?;
        let publishers_vec: Vec<DatabentoPublisher> = serde_json::from_str(&file_content)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        let (tx_cmd, rx_cmd) = std::sync::mpsc::channel::<LiveCommand>();
        let (tx_msg, rx_msg) = tokio::sync::mpsc::channel::<LiveMessage>(100_000);

        let mut feed_handler = DatabentoFeedHandler::new(
            key.clone(),
            dataset.clone(),
            rx_cmd,
            tx_msg,
            publisher_venue_map,
            HashMap::new(),
        );

        let rt = get_runtime();
        rt.spawn(async move { feed_handler.run().await });

        Ok(Self {
            key,
            dataset,
            tx: tx_cmd,
            rx: Some(rx_msg),
            glbx_exchange_map: HashMap::new(),
        })
    }

    #[pyo3(name = "load_glbx_exchange_map")]
    fn py_load_glbx_exchange_map(&mut self, map: HashMap<Symbol, Venue>) -> anyhow::Result<()> {
        self.glbx_exchange_map = map.clone();
        self.send_command(LiveCommand::UpdateGlbx(map))
    }

    #[pyo3(name = "get_glbx_exchange_map")]
    fn py_get_glbx_exchange_map(&self) -> HashMap<Symbol, Venue> {
        self.glbx_exchange_map.clone()
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe(
        &mut self,
        schema: String,
        symbols: String,
        stype_in: Option<String>,
        start: Option<UnixNanos>,
    ) -> anyhow::Result<()> {
        let stype_in = stype_in.unwrap_or("raw_symbol".to_string());

        let mut sub = Subscription::builder()
            .symbols(symbols)
            .schema(dbn::Schema::from_str(&schema)?)
            .stype_in(dbn::SType::from_str(&stype_in)?)
            .build();

        if let Some(start) = start {
            let start = OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))?;
            sub.start = Some(start);
        };

        self.send_command(LiveCommand::Subscribe(sub))
    }

    #[pyo3(name = "start")]
    fn py_start<'py>(
        &mut self,
        py: Python<'py>,
        callback: PyObject,
        start: Option<UnixNanos>,
    ) -> PyResult<&'py PyAny> {
        let cmd = StartCommand {
            replay: start.is_some(),
        };
        self.send_command(LiveCommand::Start(cmd))?;

        // Consume the receiver
        let rx = self.rx.take().expect("Client already started");

        pyo3_asyncio::tokio::future_into_py(py, async move {
            Self::process_messages(rx, callback)
                .await
                .map_err(|e| e.into())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&self) -> anyhow::Result<()> {
        self.send_command(LiveCommand::Close)
    }
}

fn call_python_with_data(py: Python, callback: &PyObject, data: Data) {
    let py_obj = data_to_pycapsule(py, data);
    match callback.call1(py, (py_obj,)) {
        Ok(_) => {}
        Err(e) => eprintln!("Error on callback, {e:?}"), // Just print error for now
    };
}

fn call_python_with_instrument(py: Python, callback: &PyObject, instrument: Box<dyn Instrument>) {
    let py_obj = convert_instrument_to_pyobject(py, instrument).expect("Error creating instrument");
    match callback.call1(py, (py_obj,)) {
        Ok(_) => {}
        Err(e) => eprintln!("Error on callback, {e:?}"), // Just print error for now
    };
}
