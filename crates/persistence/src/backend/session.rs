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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    vec::IntoIter,
};

use ahash::{AHashMap, AHashSet};
use datafusion::{
    arrow::record_batch::RecordBatch,
    error::{DataFusionError, Result},
    logical_expr::expr::Sort,
    physical_plan::SendableRecordBatchStream,
    prelude::*,
};
use futures::{Stream, StreamExt};
use nautilus_common::live::get_runtime;
use nautilus_core::UnixNanos;
use nautilus_model::data::{Data, HasTsInit};
use nautilus_serialization::arrow::{
    DataStreamingError, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError, WriteStream,
};
use object_store::ObjectStore;
use parking_lot::Mutex;
use url::Url;

use super::{
    compare::Compare,
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
        l.item.ts_init().cmp(&r.item.ts_init()).reverse()
    }
}

/// Represents a failure raised by a query's underlying data stream.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The record batch stream returned an error.
    #[error("Record batch stream error: {0}")]
    Stream(#[from] DataFusionError),
    /// A record batch could not be decoded into Nautilus data.
    #[error("Record batch decode error: {0}")]
    Decode(#[from] EncodingError),
}

/// Holds the first failure observed by any of a query's batch streams.
///
/// `failed` keeps the common path off the mutex, because [`QueryResult::next`] consults the slot
/// once per merged item while loading a catalog.
#[derive(Default)]
struct ErrorSlot {
    failed: AtomicBool,
    error: Mutex<Option<QueryError>>,
}

impl ErrorSlot {
    fn record(&self, error: QueryError) {
        self.error.lock().get_or_insert(error);
        self.failed.store(true, Ordering::Release);
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn take(&self) -> Option<QueryError> {
        self.error.lock().take()
    }
}

/// Iterates the merged data of every registered query stream in ascending `ts_init` order.
///
/// A batch stream that fails stops contributing data and the failure is yielded as an error, so a
/// failed query can never be mistaken for an exhausted one.
pub struct QueryResult {
    merge: KMerge<BatchStream, Data, TsInitComparator>,
    error: Arc<ErrorSlot>,
}

impl QueryResult {
    /// Discards the remaining data streams without draining them.
    pub fn clear(&mut self) {
        self.merge.clear();
    }
}

impl Iterator for QueryResult {
    // Spelled out because `Result` is the DataFusion alias in this module
    type Item = std::result::Result<Data, QueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        // A failure recorded while merging the previous item ends the query, so drop the
        // remaining streams rather than returning data from an incomplete result.
        if self.error.failed()
            && let Some(e) = self.error.take()
        {
            self.clear();
            return Some(Err(e));
        }

        match self.merge.next() {
            Some(item) => Some(Ok(item)),
            // Always taken, so a failure recorded on the final poll cannot read as exhaustion
            None => self.error.take().map(Err),
        }
    }
}

/// Provides a DataFusion session and registers DataFusion queries.
///
/// The session is used to register data sources and make queries on them. A
/// query returns a Chunk of Arrow records. It is decoded and converted into
/// a Vec of data by types that implement [`DecodeDataFromRecordBatch`].
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.persistence", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")
)]
pub struct DataBackendSession {
    pub chunk_size: usize,
    pub runtime: tokio::runtime::Handle,
    session_ctx: SessionContext,
    batch_streams: Vec<BatchStream>,
    error: Arc<ErrorSlot>,
    registered_tables: AHashSet<String>,
}

impl DataBackendSession {
    /// Creates a new [`DataBackendSession`] instance.
    #[must_use]
    pub fn new(chunk_size: usize) -> Self {
        let session_cfg = SessionConfig::new()
            .set_str("datafusion.optimizer.repartition_file_scans", "false")
            .set_str("datafusion.optimizer.prefer_existing_sort", "true");
        let session_ctx = SessionContext::new_with_config(session_cfg);
        Self {
            session_ctx,
            batch_streams: Vec::default(),
            error: Arc::default(),
            chunk_size,
            runtime: get_runtime().handle().clone(),
            registered_tables: AHashSet::new(),
        }
    }

    /// Register an object store with the session context
    pub fn register_object_store(&mut self, url: &Url, object_store: Arc<dyn ObjectStore>) {
        self.session_ctx.register_object_store(url, object_store);
    }

    /// Register an object store with the session context from a URI with optional storage options.
    ///
    /// # Errors
    ///
    /// Returns an error if the object store URI cannot be normalized or the backend
    /// cannot be created.
    pub fn register_object_store_from_uri(
        &mut self,
        uri: &str,
        storage_options: Option<AHashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let location =
            crate::parquet::create_object_store_location_from_path(uri, storage_options)?;

        if let Some(root_url) = location.store_root_url().cloned() {
            self.register_object_store(&root_url, location.object_store);
        }

        Ok(())
    }

    /// Writes encoded data to a streaming sink.
    ///
    /// # Errors
    ///
    /// Returns an error if Arrow encoding or stream writing fails.
    pub fn write_data<T: EncodeToRecordBatch>(
        data: &[T],
        metadata: &AHashMap<String, String>,
        stream: &mut dyn WriteStream,
    ) -> Result<(), DataStreamingError> {
        // Convert AHashMap to HashMap for Arrow compatibility
        let metadata: std::collections::HashMap<String, String> = metadata
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let record_batch = T::encode_batch(&metadata, data)?;
        stream.write(&record_batch)?;
        Ok(())
    }

    /// Registers a Parquet file and adds a batch stream for decoding.
    ///
    /// The caller must specify `T` to indicate the kind of data expected. `table_name` is
    /// the logical name for queries; `file_path` is the Parquet path; `sql_query` defaults
    /// to `SELECT * FROM {table_name} ORDER BY ts_init` if `None`.
    ///
    /// When `custom_type_name` is `Some`, it is merged into each batch's schema metadata
    /// before decoding (as `type_name`). Use this for custom data when Parquet/DataFusion
    /// does not preserve schema metadata so the decoder can look up the type in the registry.
    ///
    /// The file data must be ordered by the `ts_init` in ascending order for this
    /// to work correctly.
    ///
    /// # Errors
    ///
    /// Returns an error if parquet registration, SQL planning, stream execution, or
    /// data decoding setup fails.
    pub fn add_file<T>(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
        custom_type_name: Option<&str>,
    ) -> Result<()>
    where
        T: DecodeDataFromRecordBatch,
    {
        // Check if table is already registered to avoid duplicates
        let is_new_table = !self.registered_tables.contains(table_name);

        if is_new_table {
            // Register the table only if it doesn't exist
            let parquet_options = ParquetReadOptions::<'_> {
                skip_metadata: Some(false),
                file_sort_order: vec![vec![Sort {
                    expr: col("ts_init"),
                    asc: true,
                    nulls_first: false,
                }]],
                ..Default::default()
            };
            super::block_on(
                &self.runtime,
                self.session_ctx
                    .register_parquet(table_name, file_path, parquet_options),
            )?;

            self.registered_tables.insert(table_name.to_string());

            // Only add batch stream for newly registered tables to avoid duplicates
            let default_query = format!("SELECT * FROM {table_name} ORDER BY ts_init");
            let sql_query = sql_query.unwrap_or(&default_query);
            let query = super::block_on(&self.runtime, self.session_ctx.sql(sql_query))?;
            let batch_stream = super::block_on(&self.runtime, query.execute_stream())?;
            self.add_batch_stream::<T>(batch_stream, custom_type_name.map(String::from));
        }

        Ok(())
    }

    /// Registers a Parquet file and executes a query, returning the raw record batches.
    ///
    /// # Errors
    ///
    /// Returns an error if parquet registration, SQL planning, stream execution, or
    /// batch collection fails.
    pub fn collect_query_batches(
        &mut self,
        table_name: &str,
        file_path: &str,
        sql_query: Option<&str>,
    ) -> Result<Vec<RecordBatch>> {
        if !self.registered_tables.contains(table_name) {
            let parquet_options = ParquetReadOptions::<'_> {
                skip_metadata: Some(false),
                file_sort_order: vec![vec![Sort {
                    expr: col("ts_init"),
                    asc: true,
                    nulls_first: false,
                }]],
                ..Default::default()
            };
            super::block_on(
                &self.runtime,
                self.session_ctx
                    .register_parquet(table_name, file_path, parquet_options),
            )?;

            self.registered_tables.insert(table_name.to_string());
        }

        let default_query = format!("SELECT * FROM {table_name} ORDER BY ts_init");
        let sql_query = sql_query.unwrap_or(&default_query);
        let query = super::block_on(&self.runtime, self.session_ctx.sql(sql_query))?;
        let mut batch_stream = super::block_on(&self.runtime, query.execute_stream())?;

        super::block_on(&self.runtime, async {
            let mut batches = Vec::new();
            while let Some(batch) = batch_stream.next().await {
                batches.push(batch?);
            }
            Ok::<_, datafusion::error::DataFusionError>(batches)
        })
    }

    fn add_batch_stream<T>(
        &mut self,
        stream: SendableRecordBatchStream,
        custom_type_name: Option<String>,
    ) where
        T: DecodeDataFromRecordBatch,
    {
        self.batch_streams.push(BatchStream {
            inner: EagerStream::from_stream_with_runtime(
                decode_batches::<T>(stream, custom_type_name),
                self.runtime.clone(),
            ),
            error: Arc::clone(&self.error),
        });
    }

    // Consumes the registered queries and returns a [`QueryResult].
    // Passes the output of the query though the a KMerge which sorts the
    // queries in ascending order of `ts_init`.
    // QueryResult is an iterator that return Vec<Data>.
    pub fn get_query_result(&mut self) -> QueryResult {
        let mut merge: KMerge<_, _, _> = KMerge::new(TsInitComparator);

        self.batch_streams
            .drain(..)
            .for_each(|batch_stream| merge.push_iter(batch_stream));

        QueryResult {
            merge,
            error: std::mem::take(&mut self.error),
        }
    }

    /// Clears all registered tables and batch streams.
    ///
    /// This is useful when the underlying files have changed and we need to
    /// re-register tables with updated data.
    pub fn clear_registered_tables(&mut self) {
        self.registered_tables.clear();
        self.batch_streams.clear();
        self.error = Arc::default();

        // Create a new session context to completely reset the DataFusion state
        let session_cfg = SessionConfig::new()
            .set_str("datafusion.optimizer.repartition_file_scans", "false")
            .set_str("datafusion.optimizer.prefer_existing_sort", "true");
        self.session_ctx = SessionContext::new_with_config(session_cfg);
    }
}

type BatchResult = std::result::Result<IntoIter<Data>, QueryError>;

/// Decodes each record batch, yielding the first failure and then ending the stream.
///
/// A record batch stream that has returned an error gives no guarantee about being polled again,
/// and a panic in the producer task would abort the process under `panic = "abort"`.
fn decode_batches<T>(
    stream: SendableRecordBatchStream,
    custom_type_name: Option<String>,
) -> impl Stream<Item = BatchResult> + Send + 'static
where
    T: DecodeDataFromRecordBatch,
{
    futures::stream::unfold(
        (stream, custom_type_name, false),
        |(mut stream, custom_type_name, failed)| async move {
            if failed {
                return None;
            }

            let batch = decode_batch::<T>(stream.next().await?, custom_type_name.as_deref());
            let failed = batch.is_err();

            Some((batch, (stream, custom_type_name, failed)))
        },
    )
}

fn decode_batch<T>(
    result: std::result::Result<RecordBatch, DataFusionError>,
    custom_type_name: Option<&str>,
) -> BatchResult
where
    T: DecodeDataFromRecordBatch,
{
    let batch = result?;
    let mut metadata: std::collections::HashMap<String, String> = batch.schema().metadata().clone();

    if let Some(type_name) = custom_type_name {
        metadata.insert("type_name".to_string(), type_name.to_string());
    }

    Ok(T::decode_data_batch(&metadata, batch)?.into_iter())
}

/// Feeds decoded batches to the merge and diverts a failure to the shared error slot.
///
/// The merge orders items by `ts_init`, so a failure cannot travel with the data. Recording it
/// here ends this stream for the merge while leaving the remaining streams intact, and lets
/// [`QueryResult`] report the failure instead of exhaustion.
struct BatchStream {
    inner: EagerStream<BatchResult>,
    error: Arc<ErrorSlot>,
}

impl Iterator for BatchStream {
    type Item = IntoIter<Data>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()? {
            Ok(batch) => Some(batch),
            Err(e) => {
                self.error.record(e);
                None
            }
        }
    }
}

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
    pyo3::pyclass(module = "nautilus_trader.persistence", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")
)]
pub struct DataQueryResult {
    pub result: QueryResult,
    pub acc: Vec<Data>,
    pub size: usize,
}

impl DataQueryResult {
    /// Creates a new [`DataQueryResult`] instance.
    #[must_use]
    pub const fn new(result: QueryResult, size: usize) -> Self {
        Self {
            result,
            acc: Vec::new(),
            size,
        }
    }
}

impl Iterator for DataQueryResult {
    // An empty chunk signals exhaustion, so a failure must be reported as an error
    type Item = std::result::Result<Vec<Data>, QueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Poll at least once, since a zero chunk size would return an empty chunk without ever
        // consulting the query, hiding a failure behind the exhaustion signal.
        let size = self.size.max(1);

        for _ in 0..size {
            match self.result.next() {
                Some(Ok(item)) => self.acc.push(item),
                Some(Err(e)) => {
                    self.acc.clear();
                    return Some(Err(e));
                }
                None => break,
            }
        }

        // TODO: consider using drain here if perf is unchanged
        // Some(self.acc.drain(0..).collect())
        let mut acc: Vec<Data> = Vec::new();
        std::mem::swap(&mut acc, &mut self.acc);
        Some(Ok(acc))
    }
}

impl Drop for DataQueryResult {
    fn drop(&mut self) {
        self.result.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::atomic::AtomicUsize, task::Poll};

    use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
    use nautilus_common::live::get_runtime;
    use nautilus_model::{
        data::QuoteTick,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{
        ArrowSchemaProvider, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
    };
    #[cfg(feature = "python")]
    use pyo3::{Py, Python, exceptions::PyRuntimeError, types::PyAnyMethods};
    use rstest::rstest;

    use super::*;

    const INSTRUMENT_ID: &str = "EUR/USD.SIM";

    fn quote(ts_init: u64) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from(INSTRUMENT_ID),
            Price::from("1.0001"),
            Price::from("1.0002"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )
    }

    fn quote_metadata() -> HashMap<String, String> {
        HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), INSTRUMENT_ID.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "4".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ])
    }

    fn quote_batch(quotes: &[QuoteTick]) -> RecordBatch {
        QuoteTick::encode_batch(&quote_metadata(), quotes).expect("failed to encode quotes")
    }

    fn stream_error() -> DataFusionError {
        DataFusionError::Execution("injected stream failure".to_string())
    }

    fn batch_stream(
        batches: Vec<std::result::Result<RecordBatch, DataFusionError>>,
    ) -> SendableRecordBatchStream {
        Box::pin(RecordBatchStreamAdapter::new(
            Arc::new(QuoteTick::get_schema(Some(quote_metadata()))),
            futures::stream::iter(batches),
        ))
    }

    fn ts_inits(items: &[std::result::Result<Data, QueryError>]) -> Vec<u64> {
        items
            .iter()
            .filter_map(|item| item.as_ref().ok())
            .map(|data| data.ts_init().as_u64())
            .collect()
    }

    #[rstest]
    fn data_backend_sessions_share_global_runtime() {
        let first = DataBackendSession::new(10);
        let second = DataBackendSession::new(10);

        assert_eq!(first.runtime.id(), second.runtime.id());
        assert_eq!(first.runtime.id(), get_runtime().handle().id());
    }

    #[rstest]
    fn query_result_merges_streams_in_order_then_exhausts() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![
                Ok(quote_batch(&[quote(1), quote(3)])),
                Ok(quote_batch(&[quote(5)])),
            ]),
            None,
        );
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(2), quote(4)]))]),
            None,
        );

        let mut result = session.get_query_result();
        let items: Vec<_> = result.by_ref().collect();

        assert_eq!(ts_inits(&items), vec![1, 2, 3, 4, 5]);
        assert_eq!(items.len(), 5);
        assert!(result.next().is_none());
    }

    #[rstest]
    fn query_result_reports_stream_error_after_its_data() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![
                Ok(quote_batch(&[quote(1), quote(2)])),
                Err(stream_error()),
            ]),
            None,
        );

        let mut result = session.get_query_result();
        let items: Vec<_> = result.by_ref().collect();

        assert_eq!(ts_inits(&items), vec![1, 2]);
        assert_eq!(items.len(), 3);
        assert!(
            matches!(items[2], Err(QueryError::Stream(_))),
            "expected a stream error, was {:?}",
            items[2]
        );
        assert!(result.next().is_none());
    }

    #[rstest]
    fn query_result_stops_when_one_of_many_streams_fails() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(1)])), Err(stream_error())]),
            None,
        );
        session
            .add_batch_stream::<QuoteTick>(batch_stream(vec![Ok(quote_batch(&[quote(2)]))]), None);

        let mut result = session.get_query_result();
        let items: Vec<_> = result.by_ref().collect();

        // The healthy stream still holds `quote(2)`, so exhaustion here would look successful
        assert_eq!(ts_inits(&items), vec![1]);
        assert_eq!(items.len(), 2);
        assert!(
            matches!(items[1], Err(QueryError::Stream(_))),
            "expected a stream error, was {:?}",
            items[1]
        );
        assert!(result.next().is_none());
    }

    #[rstest]
    fn query_result_reports_decode_error() {
        let mut session = DataBackendSession::new(10);
        // Encode without schema metadata so the decoder cannot resolve the instrument
        let batch =
            QuoteTick::encode_batch(&HashMap::new(), &[quote(1)]).expect("failed to encode quotes");
        session.add_batch_stream::<QuoteTick>(batch_stream(vec![Ok(batch)]), None);

        let items: Vec<_> = session.get_query_result().collect();

        assert_eq!(items.len(), 1);
        assert!(
            matches!(
                items[0],
                Err(QueryError::Decode(EncodingError::MissingMetadata(
                    KEY_INSTRUMENT_ID
                )))
            ),
            "expected a decode error, was {:?}",
            items[0]
        );
    }

    #[rstest]
    fn data_query_result_reports_error_instead_of_an_empty_chunk() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(1)])), Err(stream_error())]),
            None,
        );

        let mut result = DataQueryResult::new(session.get_query_result(), 10);
        let chunk = result.next().expect("chunked result must yield an item");

        assert!(
            matches!(chunk, Err(QueryError::Stream(_))),
            "expected a stream error, was {chunk:?}"
        );

        let after = result
            .next()
            .expect("chunked result must signal exhaustion")
            .expect("a failed query must not fail twice");

        assert!(after.is_empty(), "the discarded chunk must not be replayed");
    }

    #[rstest]
    fn data_query_result_reports_an_error_with_a_zero_chunk_size() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(batch_stream(vec![Err(stream_error())]), None);

        let mut result = DataQueryResult::new(session.get_query_result(), 0);
        let chunk = result.next().expect("chunked result must yield an item");

        assert!(
            matches!(chunk, Err(QueryError::Stream(_))),
            "expected a stream error, was {chunk:?}"
        );
    }

    #[rstest]
    fn data_query_result_ends_with_an_empty_chunk_when_successful() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(1), quote(2)]))]),
            None,
        );

        let mut result = DataQueryResult::new(session.get_query_result(), 10);
        let chunk = result
            .next()
            .expect("chunked result must yield a chunk")
            .expect("query must not fail");

        assert_eq!(chunk.len(), 2);
        assert_eq!(
            chunk
                .iter()
                .map(|data| data.ts_init().as_u64())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        let last = result
            .next()
            .expect("chunked result must signal exhaustion")
            .expect("query must not fail");

        assert!(last.is_empty());
    }

    #[rstest]
    fn decode_batches_stops_polling_a_failed_stream() {
        let polls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&polls);
        let mut batches = vec![Ok(quote_batch(&[quote(1)])), Err(stream_error())].into_iter();
        let inner = futures::stream::poll_fn(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(batches.next())
        });
        let stream = Box::pin(RecordBatchStreamAdapter::new(
            Arc::new(QuoteTick::get_schema(Some(quote_metadata()))),
            inner,
        ));

        let decoded = decode_batches::<QuoteTick>(stream, None);
        let items: Vec<_> = futures::executor::block_on_stream(Box::pin(decoded)).collect();

        assert_eq!(items.len(), 2);
        assert!(items[0].is_ok());
        assert!(matches!(items[1], Err(QueryError::Stream(_))));
        assert_eq!(polls.load(Ordering::SeqCst), 2);
    }

    #[rstest]
    fn a_new_query_does_not_inherit_an_earlier_failure() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(batch_stream(vec![Err(stream_error())]), None);

        // Held open so a shared error slot would leak into the query registered next
        let failed = session.get_query_result();

        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(1), quote(2)]))]),
            None,
        );
        let items: Vec<_> = session.get_query_result().collect();

        let failed: Vec<_> = failed.collect();

        assert_eq!(ts_inits(&items), vec![1, 2]);
        assert_eq!(items.len(), 2);
        assert_eq!(failed.len(), 1);
        assert!(
            matches!(failed[0], Err(QueryError::Stream(_))),
            "expected a stream error, was {:?}",
            failed[0]
        );
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn python_to_list_raises_on_stream_error() {
        let mut session = DataBackendSession::new(10);
        session.add_batch_stream::<QuoteTick>(
            batch_stream(vec![Ok(quote_batch(&[quote(1)])), Err(stream_error())]),
            None,
        );
        let result = DataQueryResult::new(session.get_query_result(), 10);

        Python::initialize();
        Python::attach(|py| {
            let result = Py::new(py, result).expect("failed to create the query result");
            let error = result
                .bind(py)
                .call_method0("to_list")
                .expect_err("to_list must raise when a stream fails");

            assert!(error.is_instance_of::<PyRuntimeError>(py));
            assert!(
                error.to_string().contains("Record batch stream error"),
                "was {error}"
            );
        });
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn python_next_raises_on_decode_error() {
        let mut session = DataBackendSession::new(10);
        // Encode without schema metadata so the decoder cannot resolve the instrument
        let batch =
            QuoteTick::encode_batch(&HashMap::new(), &[quote(1)]).expect("failed to encode quotes");
        session.add_batch_stream::<QuoteTick>(batch_stream(vec![Ok(batch)]), None);
        let result = DataQueryResult::new(session.get_query_result(), 10);

        Python::initialize();
        Python::attach(|py| {
            let result = Py::new(py, result).expect("failed to create the query result");
            let error = result
                .bind(py)
                .call_method0("__next__")
                .expect_err("__next__ must raise when a batch cannot be decoded");

            assert!(error.is_instance_of::<PyRuntimeError>(py));
            assert!(
                error.to_string().contains("Record batch decode error"),
                "was {error}"
            );
        });
    }
}
