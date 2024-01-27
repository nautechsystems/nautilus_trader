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
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use databento::live::Subscription;
use dbn::{PitSymbolMap, RType, Record, SymbolIndex};
use indexmap::IndexMap;
use log::error;
use nautilus_core::python::to_pyruntime_err;
use nautilus_core::{
    python::to_pyvalue_err,
    time::{get_atomic_clock_realtime, UnixNanos},
};
use nautilus_model::data::Data;
use nautilus_model::identifiers::instrument_id::InstrumentId;
use nautilus_model::identifiers::symbol::Symbol;
use nautilus_model::identifiers::venue::Venue;
use pyo3::prelude::*;
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::databento::parsing::parse_record;
use crate::databento::types::{DatabentoPublisher, PublisherId};

#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
pub struct DatabentoLiveClient {
    #[pyo3(get)]
    pub key: String,
    #[pyo3(get)]
    pub dataset: String,
    inner: OnceLock<Arc<Mutex<databento::LiveClient>>>,
    runtime: tokio::runtime::Runtime,
    publishers: Arc<IndexMap<PublisherId, DatabentoPublisher>>,
}

impl DatabentoLiveClient {
    async fn initialize_client(&self) -> Result<databento::LiveClient, databento::Error> {
        databento::LiveClient::builder()
            .key(&self.key)?
            .dataset(&self.dataset)
            .build()
            .await
    }

    fn get_inner_client(&self) -> Result<Arc<Mutex<databento::LiveClient>>, databento::Error> {
        if let Some(client) = self.inner.get() {
            Ok(client.clone())
        } else {
            let client = self.runtime.block_on(self.initialize_client())?;
            let arc_client = Arc::new(Mutex::new(client));
            let _ = self.inner.set(arc_client.clone());
            Ok(arc_client)
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
        let publishers = publishers_vec
            .clone()
            .into_iter()
            .map(|p| (p.publisher_id, p))
            .collect::<IndexMap<u16, DatabentoPublisher>>();

        Ok(Self {
            key,
            dataset,
            inner: OnceLock::new(),
            runtime: tokio::runtime::Runtime::new()?,
            publishers: Arc::new(publishers),
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
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
                        OffsetDateTime::from_unix_timestamp_nanos(start as i128)
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
    fn py_start<'py>(&self, py: Python<'py>, callback: PyObject) -> PyResult<&'py PyAny> {
        let arc_client = self.get_inner_client().map_err(to_pyruntime_err)?;
        let publishers = self.publishers.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            let clock = get_atomic_clock_realtime();
            let mut client = arc_client.lock().await;
            let mut symbol_map = PitSymbolMap::new();

            while let Some(record) = client.next_record().await.map_err(to_pyvalue_err)? {
                let rtype = record.rtype().expect("Invalid `rtype`");
                match rtype {
                    RType::SymbolMapping => {
                        symbol_map.on_record(record).unwrap_or_else(|_| {
                            panic!("Error updating `symbol_map` with {:?}", record)
                        });
                        continue;
                    }
                    RType::Error => {
                        eprintln!("{:?}", record); // TODO: Just print stderr for now
                        error!("{:?}", record);
                        continue;
                    }
                    RType::System => {
                        eprintln!("{:?}", record); // TODO: Just print stderr for now
                        error!("{:?}", record);
                        continue;
                    }
                    _ => {} // Fall through
                }

                let raw_symbol = symbol_map
                    .get_for_rec(&record)
                    .expect("Cannot resolve raw_symbol from `symbol_map`");

                let symbol = Symbol::from_str_unchecked(raw_symbol);
                let publisher_id = record.publisher().unwrap() as PublisherId;
                let venue_str = publishers.get(&publisher_id).unwrap().venue.as_str();
                let venue = Venue::from_str_unchecked(venue_str);

                let instrument_id = InstrumentId::new(symbol, venue);
                let ts_init = clock.get_time_ns();

                let (data, _) = parse_record(&record, rtype, instrument_id, 2, Some(ts_init))
                    .map_err(to_pyvalue_err)?;

                // TODO: Improve the efficiency of this constant GIL aquisition
                Python::with_gil(|py| {
                    let data = match data {
                        Data::Delta(delta) => delta.into_py(py),
                        Data::Depth10(depth) => depth.into_py(py),
                        Data::Quote(quote) => quote.into_py(py),
                        Data::Trade(trade) => trade.into_py(py),
                        _ => panic!("Invalid data element, was {:?}", data),
                    };

                    match callback.call1(py, (data,)) {
                        Ok(_) => {}
                        Err(e) => eprintln!("Error on callback, {:?}", e), // Just print error for now
                    };
                })
            }
            Ok(())
        })
    }

    // TODO: Close wants to take ownership which isn't possible?
    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<&'py PyAny> {
        // let arc_client = self.get_inner_client().map_err(to_pyvalue_err)?;

        pyo3_asyncio::tokio::future_into_py(py, async move {
            // let client = arc_client.lock_owned().await;
            // client.close().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }
}
