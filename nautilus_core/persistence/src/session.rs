// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, io::Cursor, vec::IntoIter};

use compare::Compare;
use datafusion::{
    arrow::{datatypes::SchemaRef, ipc::writer::StreamWriter, record_batch::RecordBatch},
    error::Result,
    physical_plan::SendableRecordBatchStream,
    prelude::*,
};
use futures::{executor::block_on, Stream, StreamExt};
use nautilus_core::cvec::CVec;
use nautilus_model::data::{
    bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick, Data,
};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyCapsule},
};
use pyo3_asyncio::tokio::get_runtime;

use crate::{
    kmerge_batch::{KMerge, PeekElementBatchStream},
    parquet::{
        ArrowSchemaProvider, DataStreamingError, DecodeFromRecordBatch, EncodeToRecordBatch,
        NautilusDataType, WriteStream,
    },
};

#[derive(Debug, Default)]
pub struct TsInitComparator;

impl<S> Compare<PeekElementBatchStream<S, Data>> for TsInitComparator
where
    S: Stream<Item = IntoIter<Data>>,
{
    fn compare(
        &self,
        l: &PeekElementBatchStream<S, Data>,
        r: &PeekElementBatchStream<S, Data>,
    ) -> std::cmp::Ordering {
        // Max heap ordering must be reversed
        l.item.get_ts_init().cmp(&r.item.get_ts_init()).reverse()
    }
}

/// Provides a DataFusion session and registers DataFusion queries.
///
/// The session is used to register data sources and make queries on them. A
/// query returns a Chunk of Arrow records. It is decoded and converted into
/// a Vec of data by types that implement [`DecodeDataFromRecordBatch`].
#[pyclass]
pub struct DataBackendSession {
    session_ctx: SessionContext,
    batch_streams: Vec<Box<dyn Stream<Item = IntoIter<Data>> + Unpin>>,
    chunk_size: usize,
}

impl DataBackendSession {
    #[must_use]
    pub fn new(chunk_size: usize) -> Self {
        Self {
            session_ctx: SessionContext::default(),
            batch_streams: Vec::default(),
            chunk_size,
        }
    }

    pub fn write_data<T: EncodeToRecordBatch>(
        data: &[T],
        metadata: &HashMap<String, String>,
        stream: &mut dyn WriteStream,
    ) -> Result<(), DataStreamingError> {
        let record_batch = T::encode_batch(metadata, data);
        stream.write(&record_batch)?;
        Ok(())
    }

    // Query a file for all it's records. the caller must specify `T` to indicate
    // the kind of data expected from this query.
    pub async fn add_file_default_query<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
    ) -> Result<()>
    where
        T: DecodeFromRecordBatch + Into<Data>,
    {
        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            ..Default::default()
        };
        self.session_ctx
            .register_parquet(table_name, file_path, parquet_options)
            .await?;

        let batch_stream = self
            .session_ctx
            .sql(&format!("SELECT * FROM {} ORDER BY ts_init", &table_name))
            .await?
            .execute_stream()
            .await?;

        self.add_batch_stream::<T>(batch_stream);
        Ok(())
    }

    // Query a file for all it's records with a custom query. The caller must
    // specify `T` to indicate what kind of data is expected from this query.
    //
    // #Safety
    // They query should ensure the records are ordered by the `ts_init` field
    // in ascending order.
    pub async fn add_file_with_custom_query<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
    ) -> Result<()>
    where
        T: DecodeFromRecordBatch + Into<Data>,
    {
        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            ..Default::default()
        };
        self.session_ctx
            .register_parquet(table_name, file_path, parquet_options)
            .await?;

        let batch_stream = self
            .session_ctx
            .sql(sql_query)
            .await?
            .execute_stream()
            .await?;

        self.add_batch_stream::<T>(batch_stream);
        Ok(())
    }

    fn add_batch_stream<T>(&mut self, stream: SendableRecordBatchStream)
    where
        T: DecodeFromRecordBatch + Into<Data>,
    {
        let transform = stream.map(|result| match result {
            Ok(batch) => T::decode_batch(batch.schema().metadata(), batch).into_iter(),
            Err(_err) => panic!("Error getting next batch from RecordBatchStream"),
        });

        self.batch_streams.push(Box::new(transform));
    }

    // Consumes the registered queries and returns a [QueryResult].
    // Passes the output of the query though the a KMerge which sorts the
    // queries in ascending order of `ts_init`.
    // QueryResult is an iterator that return Vec<Data>.
    pub fn get_query_result(&mut self) -> QueryResult<Data> {
        // TODO: No need to kmerge if there is only one batch stream
        let mut kmerge: KMerge<_, _, _> = KMerge::new(TsInitComparator);

        Iterator::for_each(self.batch_streams.drain(..), |batch_stream| {
            block_on(kmerge.push_stream(batch_stream));
        });

        QueryResult {
            data: Box::new(kmerge.chunks(self.chunk_size)),
        }
    }

    fn record_batches_to_pybytes(
        batches: Vec<RecordBatch>,
        schema: SchemaRef,
    ) -> PyResult<Py<PyBytes>> {
        // Create a cursor to write to a byte array in memory
        let mut cursor = Cursor::new(Vec::new());

        {
            let mut writer = StreamWriter::try_new(&mut cursor, &schema)
                .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
            for batch in batches {
                writer
                    .write(&batch)
                    .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
            }

            writer
                .finish()
                .map_err(|err| PyErr::new::<PyRuntimeError, _>(format!("{}", err)))?;
        }

        let buffer = cursor.into_inner();

        Python::with_gil(|py| {
            let pybytes = PyBytes::new(py, &buffer);
            Ok(pybytes.into())
        })
    }
}

pub struct QueryResult<T = Data> {
    data: Box<dyn Stream<Item = Vec<T>> + Unpin>,
}

impl Iterator for QueryResult {
    type Item = Vec<Data>;

    fn next(&mut self) -> Option<Self::Item> {
        block_on(self.data.next())
    }
}

////////////////////////////////////////////////////////////////////////////////
/// Python API
////////////////////////////////////////////////////////////////////////////////

// Note: Intended to be used on a single python thread
unsafe impl Send for DataBackendSession {}

#[pymethods]
impl DataBackendSession {
    #[new]
    #[pyo3(signature=(chunk_size=5000))]
    #[must_use]
    pub fn new_session(chunk_size: usize) -> Self {
        // Initialize runtime here
        get_runtime();
        Self::new(chunk_size)
    }

    pub fn order_book_deltas_to_batches_bytes(
        _slf: PyRefMut<'_, Self>,
        data: Vec<OrderBookDelta>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = OrderBookDelta::get_metadata(
            &first.instrument_id,
            first.order.price.precision,
            first.order.size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|delta| OrderBookDelta::encode_batch(&metadata, &[delta]))
            .collect();

        let schema = OrderBookDelta::get_schema(metadata);

        DataBackendSession::record_batches_to_pybytes(batches, schema)
    }

    pub fn quote_ticks_to_batches_bytes(
        _slf: PyRefMut<'_, Self>,
        data: Vec<QuoteTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = QuoteTick::get_metadata(
            &first.instrument_id,
            first.bid.precision,
            first.bid_size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|quote| QuoteTick::encode_batch(&metadata, &[quote]))
            .collect();

        let schema = QuoteTick::get_schema(metadata);

        DataBackendSession::record_batches_to_pybytes(batches, schema)
    }

    pub fn trade_ticks_to_batches_bytes(
        _slf: PyRefMut<'_, Self>,
        data: Vec<TradeTick>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = TradeTick::get_metadata(
            &first.instrument_id,
            first.price.precision,
            first.size.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|trade| TradeTick::encode_batch(&metadata, &[trade]))
            .collect();

        let schema = TradeTick::get_schema(metadata);

        DataBackendSession::record_batches_to_pybytes(batches, schema)
    }

    pub fn bars_to_batches_bytes(
        _slf: PyRefMut<'_, Self>,
        data: Vec<Bar>,
    ) -> PyResult<Py<PyBytes>> {
        if data.is_empty() {
            return Err(PyErr::new::<PyValueError, _>("Data vector was empty."));
        }

        // Take first element and extract metadata
        let first = data.first().unwrap();
        let metadata = Bar::get_metadata(
            &first.bar_type,
            first.open.precision,
            first.volume.precision,
        );

        // Encode data to record batches
        let batches: Vec<RecordBatch> = data
            .into_iter()
            .map(|bar| Bar::encode_batch(&metadata, &[bar]))
            .collect();

        let schema = TradeTick::get_schema(metadata);

        DataBackendSession::record_batches_to_pybytes(batches, schema)
    }

    pub fn add_file(
        mut slf: PyRefMut<'_, Self>,
        table_name: &str,
        file_path: &str,
        data_type: NautilusDataType,
    ) {
        let rt = get_runtime();
        let _guard = rt.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => {
                match block_on(slf.add_file_default_query::<OrderBookDelta>(table_name, file_path))
                {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::QuoteTick => {
                match block_on(slf.add_file_default_query::<QuoteTick>(table_name, file_path)) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::TradeTick => {
                match block_on(slf.add_file_default_query::<TradeTick>(table_name, file_path)) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::Bar => {
                match block_on(slf.add_file_default_query::<Bar>(table_name, file_path)) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
        }
    }

    pub fn add_file_with_query(
        mut slf: PyRefMut<'_, Self>,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
        data_type: NautilusDataType,
    ) {
        let rt = get_runtime();
        let _guard = rt.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => {
                match block_on(
                    slf.add_file_with_custom_query::<OrderBookDelta>(
                        table_name, file_path, sql_query,
                    ),
                ) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::QuoteTick => {
                match block_on(
                    slf.add_file_with_custom_query::<QuoteTick>(table_name, file_path, sql_query),
                ) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::TradeTick => {
                match block_on(
                    slf.add_file_with_custom_query::<TradeTick>(table_name, file_path, sql_query),
                ) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
            NautilusDataType::Bar => {
                match block_on(
                    slf.add_file_with_custom_query::<Bar>(table_name, file_path, sql_query),
                ) {
                    Ok(_) => (),
                    Err(err) => panic!("Failed new_query with error {err}"),
                }
            }
        }
    }

    #[must_use]
    pub fn to_query_result(mut slf: PyRefMut<'_, Self>) -> DataQueryResult {
        let rt = get_runtime();
        let _guard = rt.enter();

        let query_result = slf.get_query_result();
        DataQueryResult::new(query_result)
    }
}

#[pyclass]
pub struct DataQueryResult {
    result: QueryResult<Data>,
    chunk: Option<CVec>,
}

#[pymethods]
impl DataQueryResult {
    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.drop_chunk();

        let rt = get_runtime();
        let _guard = rt.enter();

        slf.result.next().map(|chunk| {
            let cvec = chunk.into();
            Python::with_gil(|py| PyCapsule::new::<CVec>(py, cvec, None).unwrap().into_py(py))
        })
    }
}

// Note: Intended to be used on a single python thread
unsafe impl Send for DataQueryResult {}

impl DataQueryResult {
    fn new(result: QueryResult<Data>) -> Self {
        Self {
            result,
            chunk: None,
        }
    }

    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory. Current chunk is held by the reader,
    /// drop if exists and reset the field.
    fn drop_chunk(&mut self) {
        if let Some(CVec { ptr, len, cap }) = self.chunk.take() {
            let data: Vec<Data> =
                unsafe { Vec::from_raw_parts(ptr.cast::<nautilus_model::data::Data>(), len, cap) };
            drop(data);
        }
    }
}

impl Drop for DataQueryResult {
    fn drop(&mut self) {
        self.drop_chunk();
    }
}
