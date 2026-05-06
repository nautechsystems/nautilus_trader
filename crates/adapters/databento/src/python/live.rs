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

use std::{fmt::Debug, fs, path::PathBuf, str::FromStr, sync::Arc};

use databento::{dbn, live::Subscription};
use indexmap::IndexMap;
use nautilus_core::{
    AtomicMap,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol, Venue},
    python::{data::data_to_pycapsule, instruments::instrument_any_to_pyobject},
};
use pyo3::prelude::*;
use time::OffsetDateTime;

use super::types::DatabentoSubscriptionAck;
use crate::{
    common::Credential,
    live::{DatabentoFeedHandler, DatabentoMessage, HandlerCommand},
    symbology::{check_consistent_symbology, infer_symbology_type},
    types::DatabentoPublisher,
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.databento")
)]
pub struct DatabentoLiveClient {
    credential: Credential,
    #[pyo3(get)]
    pub dataset: String,
    is_running: bool,
    is_closed: bool,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    cmd_rx: Option<tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>>,
    buffer_size: usize,
    publisher_venue_map: IndexMap<u16, Venue>,
    symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
    reconnect_timeout_mins: Option<u64>,
}

impl Debug for DatabentoLiveClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DatabentoLiveClient))
            .field("credential", &self.credential)
            .field("dataset", &self.dataset)
            .field("is_running", &self.is_running)
            .field("is_closed", &self.is_closed)
            .finish()
    }
}

impl DatabentoLiveClient {
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.cmd_tx.is_closed()
    }

    async fn process_messages(
        mut msg_rx: tokio::sync::mpsc::Receiver<DatabentoMessage>,
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

    fn send_command(&self, cmd: HandlerCommand) -> PyResult<()> {
        self.cmd_tx.send(cmd).map_err(to_pyruntime_err)
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
    /// # Errors
    ///
    /// Returns a `PyErr` if reading or parsing the publishers file fails.
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
        let publishers_json = fs::read_to_string(publishers_filepath).map_err(to_pyvalue_err)?;
        let publishers_vec: Vec<DatabentoPublisher> =
            serde_json::from_str(&publishers_json).map_err(to_pyvalue_err)?;
        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        // Hardcoded to a reasonable size for now
        let buffer_size = 100_000;

        // Convert i64 to u64: None/negative = infinite retries, 0 = no retries, positive = timeout in minutes
        let reconnect_timeout_mins = reconnect_timeout_mins
            .and_then(|mins| if mins >= 0 { Some(mins as u64) } else { None });

        Ok(Self {
            credential: Credential::new(key),
            dataset,
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            buffer_size,
            is_running: false,
            is_closed: false,
            publisher_venue_map,
            symbol_venue_map: Arc::new(AtomicMap::new()),
            use_exchange_as_venue,
            bars_timestamp_on_close: bars_timestamp_on_close.unwrap_or(true),
            reconnect_timeout_mins,
        })
    }

    #[pyo3(name = "is_running")]
    const fn py_is_running(&self) -> bool {
        self.is_running
    }

    #[pyo3(name = "is_closed")]
    const fn py_is_closed(&self) -> bool {
        self.is_closed
    }

    #[pyo3(name = "subscribe")]
    #[pyo3(signature = (schema, instrument_ids, start=None, snapshot=None, price_precisions=None, stype_in=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_subscribe(
        &mut self,
        schema: String,
        instrument_ids: Vec<InstrumentId>,
        start: Option<u64>,
        snapshot: Option<bool>,
        price_precisions: Option<Vec<Option<u8>>>,
        stype_in: Option<String>,
    ) -> PyResult<()> {
        self.symbol_venue_map.rcu(|m| {
            for id in &instrument_ids {
                m.entry(id.symbol).or_insert(id.venue);
            }
        });

        if let Some(precisions) = price_precisions {
            if precisions.len() != instrument_ids.len() {
                return Err(to_pyvalue_err(format!(
                    "`price_precisions` length ({}) must match `instrument_ids` length ({})",
                    precisions.len(),
                    instrument_ids.len()
                )));
            }

            for (instrument_id, precision) in instrument_ids.iter().zip(precisions) {
                if let Some(precision) = precision {
                    self.send_command(HandlerCommand::SetPricePrecision(
                        instrument_id.symbol,
                        precision,
                    ))?;
                }
            }
        }
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|id| id.symbol.to_string())
            .collect();
        let first_symbol = symbols
            .first()
            .ok_or_else(|| to_pyvalue_err("No symbols provided"))?;
        let stype_in = match stype_in {
            Some(stype_in) => dbn::SType::from_str(&stype_in).map_err(to_pyvalue_err)?,
            None => infer_symbology_type(first_symbol),
        };
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let mut sub = Subscription::builder()
            .symbols(symbols)
            .schema(dbn::Schema::from_str(&schema).map_err(to_pyvalue_err)?)
            .stype_in(stype_in)
            .build();

        if let Some(start) = start {
            let start = OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))
                .map_err(to_pyvalue_err)?;
            sub.start = Some(start);
        }
        sub.use_snapshot = snapshot.unwrap_or(false);

        self.send_command(HandlerCommand::Subscribe(sub))
    }

    #[pyo3(name = "start")]
    fn py_start<'py>(
        &mut self,
        py: Python<'py>,
        callback: Py<PyAny>,
        callback_pyo3: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.is_closed {
            return Err(to_pyruntime_err("Client already closed"));
        }

        if self.is_running {
            return Err(to_pyruntime_err("Client already running"));
        }

        log::debug!("Starting client");

        self.is_running = true;

        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<DatabentoMessage>(self.buffer_size);

        // Consume the receiver
        // We guard the client from being started more than once with the
        // `is_running` flag, so here it is safe to unwrap the command receiver.
        let cmd_rx = self
            .cmd_rx
            .take()
            .ok_or_else(|| to_pyruntime_err("Command receiver already taken"))?;

        let mut feed_handler = DatabentoFeedHandler::new(
            self.credential.clone(),
            self.dataset.clone(),
            cmd_rx,
            msg_tx,
            self.publisher_venue_map.clone(),
            self.symbol_venue_map.clone(),
            self.use_exchange_as_venue,
            self.bars_timestamp_on_close,
            self.reconnect_timeout_mins,
        );

        self.send_command(HandlerCommand::Start)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (proc_handle, feed_handle) = tokio::join!(
                Self::process_messages(msg_rx, callback, callback_pyo3),
                feed_handler.run(),
            );

            match proc_handle {
                Ok(()) => log::debug!("Message processor completed"),
                Err(e) => log::error!("Message processor error: {e}"),
            }

            match feed_handle {
                Ok(()) => log::debug!("Feed handler completed"),
                Err(e) => log::error!("Feed handler error: {e}"),
            }

            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) -> PyResult<()> {
        if !self.is_running {
            return Err(to_pyruntime_err("Client never started"));
        }

        if self.is_closed {
            return Err(to_pyruntime_err("Client already closed"));
        }

        log::debug!("Closing client");

        if !self.is_closed() {
            self.send_command(HandlerCommand::Close)?;
        }

        self.is_running = false;
        self.is_closed = true;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use super::*;

    fn client() -> DatabentoLiveClient {
        DatabentoLiveClient::py_new(
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
    fn test_py_subscribe_uses_explicit_parent_stype() {
        let mut client = client();

        client
            .py_subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("parent".to_string()),
            )
            .unwrap();

        let command = client.cmd_rx.as_mut().unwrap().try_recv().unwrap();
        match command {
            HandlerCommand::Subscribe(sub) => {
                assert_eq!(sub.schema, dbn::Schema::Definition);
                assert_eq!(sub.stype_in, dbn::SType::Parent);
                assert_eq!(sub.symbols.to_api_string(), "ES.FUT");
            }
            other => panic!("expected HandlerCommand::Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    fn test_py_subscribe_rejects_invalid_stype() {
        Python::initialize();
        let mut client = client();

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

        assert!(err.to_string().contains("not-a-stype"));
        assert!(matches!(
            client.cmd_rx.as_mut().unwrap().try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
}
