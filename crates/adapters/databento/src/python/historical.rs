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

use std::{
    collections::HashMap,
    fs,
    num::NonZeroU64,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, RwLock},
};

use databento::{
    dbn::{self},
    historical::timeseries::GetRangeParams,
};
use indexmap::IndexMap;
use nautilus_core::{
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, Data, InstrumentStatus, QuoteTick, TradeTick},
    enums::BarAggregation,
    identifiers::{InstrumentId, Symbol, Venue},
    python::instruments::instrument_any_to_pyobject,
    types::Currency,
};
use pyo3::{
    IntoPyObjectExt,
    exceptions::PyException,
    prelude::*,
    types::{PyDict, PyList},
};
use tokio::sync::Mutex;

use crate::{
    common::get_date_time_range,
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg, decode_record, decode_statistics_msg,
        decode_status_msg,
    },
    symbology::{
        MetadataCache, check_consistent_symbology, decode_nautilus_instrument_id,
        infer_symbology_type, instrument_id_to_symbol_string,
    },
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, PublisherId},
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Debug)]
pub struct DatabentoHistoricalClient {
    #[pyo3(get)]
    pub key: String,
    clock: &'static AtomicTime,
    inner: Arc<Mutex<databento::HistoricalClient>>,
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
    symbol_venue_map: Arc<RwLock<HashMap<Symbol, Venue>>>,
    use_exchange_as_venue: bool,
}

#[pymethods]
impl DatabentoHistoricalClient {
    #[new]
    fn py_new(
        key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
    ) -> PyResult<Self> {
        let client = databento::HistoricalClient::builder()
            .key(key.clone())
            .map_err(to_pyvalue_err)?
            .build()
            .map_err(to_pyvalue_err)?;

        let file_content = fs::read_to_string(publishers_filepath)?;
        let publishers_vec: Vec<DatabentoPublisher> =
            serde_json::from_str(&file_content).map_err(to_pyvalue_err)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            inner: Arc::new(Mutex::new(client)),
            publisher_venue_map: Arc::new(publisher_venue_map),
            symbol_venue_map: Arc::new(RwLock::new(HashMap::new())),
            key,
            use_exchange_as_venue,
        })
    }

    #[pyo3(name = "get_dataset_range")]
    fn py_get_dataset_range<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let response = client.metadata().get_dataset_range(&dataset).await;
            match response {
                Ok(res) => Python::with_gil(|py| {
                    let dict = PyDict::new(py);
                    dict.set_item("start", res.start.to_string())?;
                    dict.set_item("end", res.end.to_string())?;
                    dict.into_py_any(py)
                }),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling response: {e}"
                ))),
            }
        })
    }

    #[pyo3(name = "get_range_instruments")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_instruments<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Definition)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();
        let use_exchange_as_venue = self.use_exchange_as_venue;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            decoder.set_upgrade_policy(dbn::VersionUpgradePolicy::UpgradeToV2);

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut instruments = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::InstrumentDefMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let mut instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                if use_exchange_as_venue && instrument_id.venue == Venue::GLBX() {
                    let exchange = msg.exchange().unwrap();
                    let venue = Venue::from_code(exchange)
                        .unwrap_or_else(|_| panic!("`Venue` not found for exchange {exchange}"));
                    instrument_id.venue = venue;
                }

                let result = decode_instrument_def_msg(msg, instrument_id, ts_init);
                match result {
                    Ok(instrument) => instruments.push(instrument),
                    Err(e) => tracing::error!("{e:?}"),
                }
            }

            Python::with_gil(|py| {
                let py_results: PyResult<Vec<PyObject>> = instruments
                    .into_iter()
                    .map(|result| instrument_any_to_pyobject(py, result))
                    .collect();

                py_results.map(|objs| {
                    PyList::new(py, &objs)
                        .expect("Invalid `ExactSizeIterator`")
                        .into_py_any_unwrap(py)
                })
            })
        })
    }

    #[pyo3(name = "get_range_quotes")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None, price_precision=None, schema=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_quotes<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
        price_precision: Option<u8>,
        schema: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let schema = schema.unwrap_or_else(|| "mbp-1".to_string());
        let dbn_schema = dbn::Schema::from_str(&schema).map_err(to_pyvalue_err)?;
        match dbn_schema {
            dbn::Schema::Mbp1 | dbn::Schema::Bbo1S | dbn::Schema::Bbo1M => (),
            _ => {
                return Err(to_pyvalue_err(
                    "Invalid schema. Must be one of: mbp-1, bbo-1s, bbo-1m",
                ));
            }
        }
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn_schema)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);
        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<QuoteTick> = Vec::new();

            let mut process_record = |record: dbn::RecordRef| -> PyResult<()> {
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Don't include trades
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Quote(quote)) => {
                        result.push(quote);
                        Ok(())
                    }
                    _ => panic!("Invalid data element not `QuoteTick`, was {data:?}"),
                }
            };

            match dbn_schema {
                dbn::Schema::Mbp1 => {
                    while let Ok(Some(msg)) = decoder.decode_record::<dbn::Mbp1Msg>().await {
                        process_record(dbn::RecordRef::from(msg))?;
                    }
                }
                dbn::Schema::Bbo1M => {
                    while let Ok(Some(msg)) = decoder.decode_record::<dbn::Bbo1MMsg>().await {
                        process_record(dbn::RecordRef::from(msg))?;
                    }
                }
                dbn::Schema::Bbo1S => {
                    while let Ok(Some(msg)) = decoder.decode_record::<dbn::Bbo1SMsg>().await {
                        process_record(dbn::RecordRef::from(msg))?;
                    }
                }
                _ => panic!("Invalid schema {dbn_schema}"),
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }

    #[pyo3(name = "get_range_trades")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None, price_precision=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_trades<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
        price_precision: Option<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Trades)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);
        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<TradeTick> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::TradeMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Not applicable (trade will be decoded regardless)
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Trade(trade)) => {
                        result.push(trade);
                    }
                    _ => panic!("Invalid data element not `TradeTick`, was {data:?}"),
                }
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }

    #[pyo3(name = "get_range_bars")]
    #[pyo3(signature = (dataset, instrument_ids, aggregation, start, end=None, limit=None, price_precision=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_bars<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        aggregation: BarAggregation,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
        price_precision: Option<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let schema = match aggregation {
            BarAggregation::Second => dbn::Schema::Ohlcv1S,
            BarAggregation::Minute => dbn::Schema::Ohlcv1M,
            BarAggregation::Hour => dbn::Schema::Ohlcv1H,
            BarAggregation::Day => dbn::Schema::Ohlcv1D,
            _ => panic!("Invalid `BarAggregation` for request, was {aggregation}"),
        };
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(schema)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);
        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<Bar> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::OhlcvMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let (data, _) = decode_record(
                    &record,
                    instrument_id,
                    price_precision,
                    Some(ts_init),
                    false, // Not applicable
                )
                .map_err(to_pyvalue_err)?;

                match data {
                    Some(Data::Bar(bar)) => {
                        result.push(bar);
                    }
                    _ => panic!("Invalid data element not `Bar`, was {data:?}"),
                }
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }

    #[pyo3(name = "get_range_imbalance")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None, price_precision=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_imbalance<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
        price_precision: Option<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Imbalance)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);
        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<DatabentoImbalance> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::ImbalanceMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let imbalance = decode_imbalance_msg(msg, instrument_id, price_precision, ts_init)
                    .map_err(to_pyvalue_err)?;

                result.push(imbalance);
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }

    #[pyo3(name = "get_range_statistics")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None, price_precision=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_statistics<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
        price_precision: Option<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Statistics)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = price_precision.unwrap_or(Currency::USD().precision);
        let publisher_venue_map = self.publisher_venue_map.clone();
        let symbol_venue_map = self.symbol_venue_map.clone();
        let ts_init = self.clock.get_time_ns();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<DatabentoStatistics> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::StatMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let statistics =
                    decode_statistics_msg(msg, instrument_id, price_precision, ts_init)
                        .map_err(to_pyvalue_err)?;

                result.push(statistics);
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }

    #[pyo3(name = "get_range_status")]
    #[pyo3(signature = (dataset, instrument_ids, start, end=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_get_range_status<'py>(
        &self,
        py: Python<'py>,
        dataset: String,
        instrument_ids: Vec<InstrumentId>,
        start: u64,
        end: Option<u64>,
        limit: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let mut symbol_venue_map = self.symbol_venue_map.write().unwrap();
        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        let stype_in = infer_symbology_type(symbols.first().unwrap());
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice()).map_err(to_pyvalue_err)?;
        let end = end.unwrap_or(self.clock.get_time_ns().as_u64());
        let time_range = get_date_time_range(start.into(), end.into()).map_err(to_pyvalue_err)?;
        let params = GetRangeParams::builder()
            .dataset(dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Status)
            .limit(limit.and_then(NonZeroU64::new))
            .build();

        let publisher_venue_map = self.publisher_venue_map.clone();
        let ts_init = self.clock.get_time_ns();
        let symbol_venue_map = self.symbol_venue_map.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = client.lock().await; // TODO: Use a client pool
            let mut decoder = client
                .timeseries()
                .get_range(&params)
                .await
                .map_err(to_pyvalue_err)?;

            let metadata = decoder.metadata().clone();
            let mut metadata_cache = MetadataCache::new(metadata);
            let mut result: Vec<InstrumentStatus> = Vec::new();

            while let Ok(Some(msg)) = decoder.decode_record::<dbn::StatusMsg>().await {
                let record = dbn::RecordRef::from(msg);
                let instrument_id = decode_nautilus_instrument_id(
                    &record,
                    &mut metadata_cache,
                    &publisher_venue_map,
                    &symbol_venue_map.read().unwrap(),
                )
                .map_err(to_pyvalue_err)?;

                let status =
                    decode_status_msg(msg, instrument_id, ts_init).map_err(to_pyvalue_err)?;

                result.push(status);
            }

            Python::with_gil(|py| result.into_py_any(py))
        })
    }
}
