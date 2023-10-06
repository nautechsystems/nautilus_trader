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

use std::{collections::HashMap, vec::IntoIter};

use compare::Compare;
use datafusion::{error::Result, physical_plan::SendableRecordBatchStream, prelude::*};
use futures::{executor::block_on, StreamExt};
use nautilus_core::{cvec::CVec, python::to_pyruntime_err};
use nautilus_model::data::{
    bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick, Data, HasTsInit,
};
use pyo3::{prelude::*, types::PyCapsule};
use pyo3_asyncio::tokio::get_runtime;

use crate::{
    arrow::{
        DataStreamingError, DecodeDataFromRecordBatch, EncodeToRecordBatch, NautilusDataType,
        WriteStream,
    },
    kmerge_batch::{EagerStream, ElementBatchIter, KMerge},
};

#[derive(Debug, Default)]
pub struct TsInitComparator;

impl<I> Compare<ElementBatchIter<I, Data>> for TsInitComparator
where
    I: Iterator<Item = IntoIter<Data>>,
{
    fn compare(
        &self,
        l: &ElementBatchIter<I, Data>,
        r: &ElementBatchIter<I, Data>,
    ) -> std::cmp::Ordering {
        // Max heap ordering must be reversed
        l.item.get_ts_init().cmp(&r.item.get_ts_init()).reverse()
    }
}

pub type QueryResult = KMerge<EagerStream<std::vec::IntoIter<Data>>, Data, TsInitComparator>;

/// Provides a DataFusion session and registers DataFusion queries.
///
/// The session is used to register data sources and make queries on them. A
/// query returns a Chunk of Arrow records. It is decoded and converted into
/// a Vec of data by types that implement [`DecodeFromRecordBatch`].
#[pyclass]
pub struct DataBackendSession {
    session_ctx: SessionContext,
    batch_streams: Vec<EagerStream<IntoIter<Data>>>,
    pub chunk_size: usize,
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
        let record_batch = T::encode_batch(metadata, data)?;
        stream.write(&record_batch)?;
        Ok(())
    }

    /// Query a file for all it's records. the caller must specify `T` to indicate
    /// the kind of data expected from this query.
    ///
    /// # Safety
    /// The file data must be ordered by the ts_init in ascending order for this
    /// to work correctly.
    pub async fn add_file_default_query<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
    ) -> Result<()>
    where
        T: DecodeDataFromRecordBatch + Into<Data>,
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
            .sql(&format!("SELECT * FROM {}", &table_name))
            .await?
            .execute_stream()
            .await?;

        self.add_batch_stream::<T>(batch_stream);
        Ok(())
    }

    // Query a file for all it's records with a custom query. The caller must
    // specify `T` to indicate what kind of data is expected from this query.
    //
    // # Safety
    // The query should ensure the records are ordered by the `ts_init` field
    // in ascending order.
    pub async fn add_file_with_custom_query<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
    ) -> Result<()>
    where
        T: DecodeDataFromRecordBatch + Into<Data>,
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
        T: DecodeDataFromRecordBatch + Into<Data>,
    {
        let transform = stream.map(|result| match result {
            Ok(batch) => T::decode_data_batch(batch.schema().metadata(), batch)
                .unwrap()
                .into_iter(),
            Err(_err) => panic!("Error getting next batch from RecordBatchStream"),
        });

        self.batch_streams.push(EagerStream::from_stream(transform));
    }

    // Consumes the registered queries and returns a [`QueryResult].
    // Passes the output of the query though the a KMerge which sorts the
    // queries in ascending order of `ts_init`.
    // QueryResult is an iterator that return Vec<Data>.
    pub fn get_query_result(&mut self) -> QueryResult {
        let mut kmerge: KMerge<_, _, _> = KMerge::new(TsInitComparator);

        self.batch_streams
            .drain(..)
            .for_each(|eager_stream| kmerge.push_iter(eager_stream));

        kmerge
    }
}

// Note: Intended to be used on a single python thread
unsafe impl Send for DataBackendSession {}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl DataBackendSession {
    #[new]
    #[pyo3(signature=(chunk_size=5_000))]
    fn new_session(chunk_size: usize) -> Self {
        // Initialize runtime here
        get_runtime();
        Self::new(chunk_size)
    }

    fn add_file(
        mut slf: PyRefMut<'_, Self>,
        table_name: &str,
        file_path: &str,
        data_type: NautilusDataType,
    ) -> PyResult<()> {
        let rt = get_runtime();
        let _guard = rt.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => {
                block_on(slf.add_file_default_query::<OrderBookDelta>(table_name, file_path))
                    .map_err(to_pyruntime_err)
            }
            NautilusDataType::QuoteTick => {
                block_on(slf.add_file_default_query::<QuoteTick>(table_name, file_path))
                    .map_err(to_pyruntime_err)
            }
            NautilusDataType::TradeTick => {
                block_on(slf.add_file_default_query::<TradeTick>(table_name, file_path))
                    .map_err(to_pyruntime_err)
            }
            NautilusDataType::Bar => {
                block_on(slf.add_file_default_query::<Bar>(table_name, file_path))
                    .map_err(to_pyruntime_err)
            }
        }
    }

    fn add_file_with_query(
        mut slf: PyRefMut<'_, Self>,
        table_name: &str,
        file_path: &str,
        sql_query: &str,
        data_type: NautilusDataType,
    ) -> PyResult<()> {
        let rt = get_runtime();
        let _guard = rt.enter();

        match data_type {
            NautilusDataType::OrderBookDelta => block_on(
                slf.add_file_with_custom_query::<OrderBookDelta>(table_name, file_path, sql_query),
            )
            .map_err(to_pyruntime_err),
            NautilusDataType::QuoteTick => block_on(
                slf.add_file_with_custom_query::<QuoteTick>(table_name, file_path, sql_query),
            )
            .map_err(to_pyruntime_err),
            NautilusDataType::TradeTick => block_on(
                slf.add_file_with_custom_query::<TradeTick>(table_name, file_path, sql_query),
            )
            .map_err(to_pyruntime_err),
            NautilusDataType::Bar => {
                block_on(slf.add_file_with_custom_query::<Bar>(table_name, file_path, sql_query))
                    .map_err(to_pyruntime_err)
            }
        }
    }

    fn to_query_result(mut slf: PyRefMut<'_, Self>) -> DataQueryResult {
        let query_result = slf.get_query_result();
        DataQueryResult::new(query_result, slf.chunk_size)
    }
}

#[pyclass]
pub struct DataQueryResult {
    result: QueryResult,
    chunk: Option<CVec>,
    acc: Vec<Data>,
    size: usize,
}

#[cfg(feature = "python")]
#[pymethods]
impl DataQueryResult {
    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PyObject>> {
        slf.drop_chunk();

        for _ in 0..slf.size {
            match slf.result.next() {
                Some(item) => slf.acc.push(item),
                None => break,
            }
        }

        let mut acc: Vec<Data> = Vec::new();
        std::mem::swap(&mut acc, &mut slf.acc);

        let cvec = acc.into();
        Python::with_gil(|py| match PyCapsule::new::<CVec>(py, cvec, None) {
            Ok(capsule) => Ok(Some(capsule.into_py(py))),
            Err(err) => Err(to_pyruntime_err(err)),
        })
    }
}

impl DataQueryResult {
    #[must_use]
    pub fn new(result: QueryResult, size: usize) -> Self {
        Self {
            result,
            chunk: None,
            acc: Vec::new(),
            size,
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

// Note: Intended to be used on a single python thread
unsafe impl Send for DataQueryResult {}
