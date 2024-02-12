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

use std::fs;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use databento::live::Subscription;
use dbn::{PitSymbolMap, Record, SymbolIndex, VersionUpgradePolicy};
use indexmap::IndexMap;
use log::{error, info};
use nautilus_core::python::to_pyruntime_err;
use nautilus_core::time::AtomicTime;
use nautilus_core::{
    python::to_pyvalue_err,
    time::{get_atomic_clock_realtime, UnixNanos},
};
use nautilus_model::data::Data;
use nautilus_model::identifiers::instrument_id::InstrumentId;
use nautilus_model::identifiers::symbol::Symbol;
use nautilus_model::identifiers::venue::Venue;
use nautilus_model::python::data::data_to_pycapsule;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::databento::decode::{decode_instrument_def_msg, decode_record, raw_ptr_to_ustr};
use crate::databento::types::{DatabentoPublisher, PublisherId};

use super::loader::convert_instrument_to_pyobject;

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoLiveClient {
    #[pyo3(get)]
    pub key: String,
    #[pyo3(get)]
    pub dataset: String,
    inner: Option<Arc<Mutex<databento::LiveClient>>>,
    runtime: tokio::runtime::Runtime,
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
}

impl DatabentoLiveClient {
    async fn initialize_client(&self) -> Result<databento::LiveClient, databento::Error> {
        databento::LiveClient::builder()
            .key(&self.key)?
            .dataset(&self.dataset)
            .upgrade_policy(VersionUpgradePolicy::Upgrade)
            .build()
            .await
    }

    fn get_inner_client(&mut self) -> Result<Arc<Mutex<databento::LiveClient>>, databento::Error> {
        match &self.inner {
            Some(client) => Ok(client.clone()),
            None => {
                let client = self.runtime.block_on(self.initialize_client())?;
                self.inner = Some(Arc::new(Mutex::new(client)));
                Ok(self.inner.clone().unwrap())
            }
        }
    }
}

#[pymethods]
impl DatabentoLiveClient {
    #[new]
    pub fn py_new(key: String, dataset: String, publishers_path: String) -> PyResult<Self> {
        let file_content = fs::read_to_string(publishers_path)?;
        let publishers_vec: Vec<DatabentoPublisher> =
            serde_json::from_str(&file_content).map_err(to_pyvalue_err)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(Self {
            key,
            dataset,
            inner: None,
            runtime: tokio::runtime::Runtime::new()?,
            publisher_venue_map: Arc::new(publisher_venue_map),
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &mut self,
        py: Python<'py>,
        schema: String,
        symbols: String,
        stype_in: Option<String>,
        start: Option<UnixNanos>,
    ) -> PyResult<&'py PyAny> {
        let stype_in = stype_in.unwrap_or("raw_symbol".to_string());
        let arc_client = self.get_inner_client().map_err(to_pyruntime_err)?;

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut client = arc_client.lock().await;

            // TODO: This can be tidied up, conditionally calling `if let Some(start)` on
            // the builder was proving troublesome.
            let subscription = match start {
                Some(start) => Subscription::builder()
                    .symbols(symbols)
                    .schema(dbn::Schema::from_str(&schema).map_err(to_pyvalue_err)?)
                    .stype_in(dbn::SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
                    .start(
                        OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))
                            .map_err(to_pyvalue_err)?,
                    )
                    .build(),
                None => Subscription::builder()
                    .symbols(symbols)
                    .schema(dbn::Schema::from_str(&schema).map_err(to_pyvalue_err)?)
                    .stype_in(dbn::SType::from_str(&stype_in).map_err(to_pyvalue_err)?)
                    .build(),
            };

            client
                .subscribe(&subscription)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "start")]
    fn py_start<'py>(&mut self, py: Python<'py>, callback: PyObject) -> PyResult<&'py PyAny> {
        let arc_client = self.get_inner_client().map_err(to_pyruntime_err)?;
        let publisher_venue_map = self.publisher_venue_map.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let clock = get_atomic_clock_realtime();
            let mut client = arc_client.lock().await;
            let mut symbol_map = PitSymbolMap::new();

            let timeout_duration = Duration::from_millis(10);
            let relock_interval = timeout_duration.as_nanos() as u64;
            let mut lock_last_dropped_ns = 0_u64;

            client.start().await.map_err(to_pyruntime_err)?;

            loop {
                // Check if need to drop then re-aquire lock
                let now_ns = clock.get_time_ns();
                if now_ns >= lock_last_dropped_ns + relock_interval {
                    // Drop the client which will release the `MutexGuard`,
                    // allowing other futures to obtain it.
                    drop(client);

                    // Re-aquire the lock to be able to receive the next record
                    client = arc_client.lock().await;
                    lock_last_dropped_ns = now_ns;
                }

                let result = timeout(timeout_duration, client.next_record()).await;
                let record_opt = match result {
                    Ok(record_opt) => record_opt,
                    Err(_) => continue, // Timeout
                };

                let record = match record_opt {
                    Ok(Some(record)) => record,
                    Ok(None) => break, // Session ended normally
                    Err(e) => {
                        // Fail the session entirely for now. Consider refining
                        // this strategy to handle specific errors more gracefully.
                        return Err(to_pyruntime_err(e));
                    }
                };

                if let Some(msg) = record.get::<dbn::ErrorMsg>() {
                    handle_error_msg(msg);
                } else if let Some(msg) = record.get::<dbn::SystemMsg>() {
                    handle_system_msg(msg);
                } else if let Some(msg) = record.get::<dbn::SymbolMappingMsg>() {
                    handle_symbol_mapping_msg(msg, &mut symbol_map);
                } else if let Some(msg) = record.get::<dbn::InstrumentDefMsg>() {
                    handle_instrument_def_msg(msg, &publisher_venue_map, clock, &callback);
                } else {
                    handle_record(record, &symbol_map, &publisher_venue_map, clock, &callback)
                        .map_err(to_pyvalue_err)?;
                };
            }
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<&'py PyAny> {
        match self.inner.take() {
            Some(arc_client) => {
                pyo3_asyncio::tokio::future_into_py(py, async move {
                    let _client = arc_client.lock_owned().await;
                    // Still need to determine how to take ownership here
                    // client.close().await.map_err(to_pyruntime_err)
                    Ok(())
                })
            }
            None => Err(PyRuntimeError::new_err(
                "Error on close: client was never started",
            )),
        }
    }
}

fn handle_error_msg(msg: &dbn::ErrorMsg) {
    eprintln!("{msg:?}"); // TODO: Just print stderr for now
    error!("{:?}", msg);
}

fn handle_system_msg(msg: &dbn::SystemMsg) {
    println!("{msg:?}"); // TODO: Just print stdout for now
    info!("{:?}", msg);
}

fn handle_symbol_mapping_msg(msg: &dbn::SymbolMappingMsg, symbol_map: &mut PitSymbolMap) {
    symbol_map
        .on_symbol_mapping(msg)
        .unwrap_or_else(|_| panic!("Error updating `symbol_map` with {msg:?}"));
}

fn handle_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    clock: &AtomicTime,
    callback: &PyObject,
) {
    let raw_symbol = unsafe { raw_ptr_to_ustr(msg.raw_symbol.as_ptr()).unwrap() };
    let symbol = Symbol { value: raw_symbol };
    let venue = publisher_venue_map.get(&msg.hd.publisher_id).unwrap();
    let instrument_id = InstrumentId::new(symbol, *venue);

    let ts_init = clock.get_time_ns();
    let result = decode_instrument_def_msg(msg, instrument_id, ts_init);

    match result {
        Ok(instrument) => {
            Python::with_gil(|py| {
                let py_obj = convert_instrument_to_pyobject(py, instrument).unwrap();
                match callback.call1(py, (py_obj,)) {
                    Ok(_) => {}
                    Err(e) => eprintln!("Error on callback, {e:?}"), // Just print error for now
                };
            });
        }
        Err(e) => eprintln!("{e:?}"),
    }
}

fn handle_record(
    record: dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    clock: &AtomicTime,
    callback: &PyObject,
) -> Result<()> {
    let raw_symbol = symbol_map
        .get_for_rec(&record)
        .expect("Cannot resolve `raw_symbol` from `symbol_map`");
    let publisher_id = record.publisher().unwrap() as PublisherId;
    let symbol = Symbol::from_str_unchecked(raw_symbol);
    let venue = publisher_venue_map.get(&publisher_id).unwrap();
    let instrument_id = InstrumentId::new(symbol, *venue);

    let price_precision = 2; // Hard coded for now
    let ts_init = clock.get_time_ns();

    let (data1, data2) = decode_record(
        &record,
        instrument_id,
        price_precision,
        Some(ts_init),
        true, // Always include trades
    )?;

    Python::with_gil(|py| {
        if let Some(data) = data1 {
            call_python_with_data(py, callback, data);
        }

        if let Some(data) = data2 {
            call_python_with_data(py, callback, data);
        }
    });

    Ok(())
}

fn call_python_with_data(py: Python, callback: &PyObject, data: Data) {
    let py_obj = data_to_pycapsule(py, data);
    match callback.call1(py, (py_obj,)) {
        Ok(_) => {}
        Err(e) => eprintln!("Error on callback, {e:?}"), // Just print error for now
    };
}
