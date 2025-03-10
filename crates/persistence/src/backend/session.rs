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

use std::{collections::HashMap, sync::Arc, vec::IntoIter};

use compare::Compare;
use datafusion::{
    error::Result, logical_expr::expr::Sort, physical_plan::SendableRecordBatchStream, prelude::*,
};
use futures::StreamExt;
use nautilus_core::{UnixNanos, ffi::cvec::CVec};
use nautilus_model::data::{Data, GetTsInit};
use nautilus_serialization::arrow::{
    DataStreamingError, DecodeDataFromRecordBatch, EncodeToRecordBatch, WriteStream,
};

use super::kmerge_batch::{EagerStream, ElementBatchIter, KMerge};

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
        l.item.ts_init().cmp(&r.item.ts_init()).reverse()
    }
}

pub type QueryResult = KMerge<EagerStream<std::vec::IntoIter<Data>>, Data, TsInitComparator>;

/// Provides a DataFusion session and registers DataFusion queries.
///
/// The session is used to register data sources and make queries on them. A
/// query returns a Chunk of Arrow records. It is decoded and converted into
/// a Vec of data by types that implement [`DecodeDataFromRecordBatch`].
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.persistence")
)]
pub struct DataBackendSession {
    pub chunk_size: usize,
    pub runtime: Arc<tokio::runtime::Runtime>,
    session_ctx: SessionContext,
    batch_streams: Vec<EagerStream<IntoIter<Data>>>,
}

impl DataBackendSession {
    /// Creates a new [`DataBackendSession`] instance.
    #[must_use]
    pub fn new(chunk_size: usize) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let session_cfg = SessionConfig::new()
            .set_str("datafusion.optimizer.repartition_file_scans", "false")
            .set_str("datafusion.optimizer.prefer_existing_sort", "true");
        let session_ctx = SessionContext::new_with_config(session_cfg);
        Self {
            session_ctx,
            batch_streams: Vec::default(),
            chunk_size,
            runtime: Arc::new(runtime),
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

    /// Query a file for its records. the caller must specify `T` to indicate
    /// the kind of data expected from this query.
    ///
    /// `table_name`: Logical `table_name` assigned to this file. Queries to this file should address the
    /// file by its table name.
    /// `file_path`: Path to file
    /// `sql_query`: A custom sql query to retrieve records from file. If no query is provided a default
    /// query "SELECT * FROM <`table_name`>" is run.
    ///
    /// # Safety
    ///
    /// The file data must be ordered by the `ts_init` in ascending order for this
    /// to work correctly.
    pub fn add_file<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> Result<()>
    where
        T: DecodeDataFromRecordBatch + Into<Data>,
    {
        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            file_sort_order: vec![vec![Sort {
                expr: col("ts_init"),
                asc: true,
                nulls_first: false,
            }]],
            ..Default::default()
        };
        self.runtime.block_on(self.session_ctx.register_parquet(
            table_name,
            file_path,
            parquet_options,
        ))?;

        let default_query = format!("SELECT * FROM {} ORDER BY ts_init", &table_name);
        let sql_query = sql_query.unwrap_or(&default_query);
        let query = self.runtime.block_on(self.session_ctx.sql(sql_query))?;

        let batch_stream = self.runtime.block_on(query.execute_stream())?;

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
            Err(e) => panic!("Error getting next batch from RecordBatchStream: {e}"),
        });

        self.batch_streams
            .push(EagerStream::from_stream_with_runtime(
                transform,
                self.runtime.clone(),
            ));
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

// Note: Intended to be used on a single Python thread
unsafe impl Send for DataBackendSession {}

#[must_use]
pub fn build_query(
    table: &str,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let mut conditions = Vec::new();

    // Add where clause if provided
    if let Some(clause) = where_clause {
        conditions.push(clause.to_string());
    }

    // Add start condition if provided
    if let Some(start_ts) = start {
        conditions.push(format!("ts_init >= {start_ts}"));
    }

    // Add end condition if provided
    if let Some(end_ts) = end {
        conditions.push(format!("ts_init <= {end_ts}"));
    }

    // Build base query
    let mut query = format!("SELECT * FROM {table}");

    // Add WHERE clause if there are conditions
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    // Add ORDER BY clause
    query.push_str(" ORDER BY ts_init");

    query
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.persistence", unsendable)
)]
pub struct DataQueryResult {
    pub chunk: Option<CVec>,
    pub result: QueryResult,
    pub acc: Vec<Data>,
    pub size: usize,
}

impl DataQueryResult {
    /// Creates a new [`DataQueryResult`] instance.
    #[must_use]
    pub const fn new(result: QueryResult, size: usize) -> Self {
        Self {
            chunk: None,
            result,
            acc: Vec::new(),
            size,
        }
    }

    /// Set new `CVec` backed chunk from data
    ///
    /// It also drops previously allocated chunk
    pub fn set_chunk(&mut self, data: Vec<Data>) -> CVec {
        self.drop_chunk();

        let chunk: CVec = data.into();
        self.chunk = Some(chunk);
        chunk
    }

    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory. Current chunk is held by the reader,
    /// drop if exists and reset the field.
    pub fn drop_chunk(&mut self) {
        if let Some(CVec { ptr, len, cap }) = self.chunk.take() {
            let data: Vec<Data> =
                unsafe { Vec::from_raw_parts(ptr.cast::<nautilus_model::data::Data>(), len, cap) };
            drop(data);
        }
    }
}

impl Iterator for DataQueryResult {
    type Item = Vec<Data>;

    fn next(&mut self) -> Option<Self::Item> {
        for _ in 0..self.size {
            match self.result.next() {
                Some(item) => self.acc.push(item),
                None => break,
            }
        }

        // TODO: consider using drain here if perf is unchanged
        // Some(self.acc.drain(0..).collect())
        let mut acc: Vec<Data> = Vec::new();
        std::mem::swap(&mut acc, &mut self.acc);
        Some(acc)
    }
}

impl Drop for DataQueryResult {
    fn drop(&mut self) {
        self.drop_chunk();
        self.result.clear();
    }
}

// Note: Intended to be used on a single Python thread
unsafe impl Send for DataQueryResult {}
