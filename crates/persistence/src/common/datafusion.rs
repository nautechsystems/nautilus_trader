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

//! DataFusion planning and blocking execution shared by catalog backends.
#![expect(
    clippy::missing_errors_doc,
    reason = "DataFusion session methods forward query and Arrow errors for controlled schemas"
)]

use std::sync::Arc;

use ahash::AHashSet;
use datafusion::{
    arrow::{
        array::{
            Array, ArrayRef, BinaryViewArray, FixedSizeBinaryBuilder, FixedSizeListArray,
            ListArray, StringArray, StringViewArray, new_empty_array, new_null_array,
        },
        buffer::{OffsetBuffer, ScalarBuffer},
        compute::{cast, concat},
        datatypes::{DataType, Schema},
        record_batch::RecordBatch,
    },
    catalog::TableProvider,
    error::{DataFusionError, Result},
    physical_plan::{EmptyRecordBatchStream, SendableRecordBatchStream},
    prelude::*,
};
use futures::{Stream, StreamExt, TryStreamExt};
use nautilus_common::live::{block_on_nautilus_with, get_runtime};
use nautilus_core::UnixNanos;
use object_store::ObjectStore;
use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};
use url::Url;

use crate::common::{arrow::validate_catalog_schema, storage::StorageBackend};

/// Batches buffered ahead of a blocking consumer.
///
/// A depth of one would stall the producing task on every item until the consumer takes it, so the
/// object-store read of the next batches cannot overlap with decoding the current one.
const BATCH_STREAM_BUFFER: usize = 4;

pub(crate) struct BlockingBatchStream<T> {
    receiver: Receiver<T>,
    task: JoinHandle<()>,
}

impl<T> BlockingBatchStream<T> {
    pub(crate) fn from_stream_with_runtime<S>(stream: S, runtime: &tokio::runtime::Handle) -> Self
    where
        S: Stream<Item = T> + Send + 'static,
        T: Send + 'static,
    {
        let (sender, receiver) = mpsc::channel(BATCH_STREAM_BUFFER);

        let task = runtime.spawn(async move {
            futures::pin_mut!(stream);

            while let Some(item) = stream.next().await {
                if sender.send(item).await.is_err() {
                    break;
                }
            }
        });

        Self { receiver, task }
    }
}

impl<T: Send> Iterator for BlockingBatchStream<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        block_on_nautilus_with(|| self.receiver.recv())
    }
}

impl<T> Drop for BlockingBatchStream<T> {
    fn drop(&mut self) {
        self.receiver.close();
        self.task.abort();
    }
}

/// Provides a DataFusion session for registering and querying catalog table sources.
pub struct DataBackendSession {
    pub chunk_size: usize,
    pub runtime: tokio::runtime::Handle,
    pub(crate) session_ctx: SessionContext,
    pub(crate) registered_tables: AHashSet<String>,
}

impl DataBackendSession {
    /// Creates a new [`DataBackendSession`] instance.
    #[must_use]
    pub fn new(chunk_size: usize) -> Self {
        let runtime = get_runtime().handle().clone();
        let session_ctx = SessionContext::new_with_config(session_config());

        Self {
            chunk_size,
            runtime,
            session_ctx,
            registered_tables: AHashSet::new(),
        }
    }

    /// Register an object store with the session context
    pub fn register_object_store(&mut self, url: &Url, object_store: Arc<dyn ObjectStore>) {
        self.session_ctx.register_object_store(url, object_store);
    }

    /// Registers an OpenDAL-backed storage backend with the session context.
    ///
    /// External catalog implementations can call this before adding native table providers or
    /// object-store-relative file paths to the session.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage URI cannot be converted into a DataFusion root URL.
    pub fn register_storage_backend(&mut self, storage: &StorageBackend) -> anyhow::Result<()> {
        let root_url = storage.datafusion_root_url()?;
        self.register_object_store(&root_url, storage.object_store.clone());
        Ok(())
    }

    /// Registers a table provider with the session context.
    ///
    /// This supports non-Parquet table formats such as Delta Lake while keeping the Parquet
    /// registration path unchanged.
    pub fn register_table_provider(
        &mut self,
        table_name: &str,
        provider: Arc<dyn TableProvider>,
    ) -> Result<()> {
        if !self.registered_tables.contains(table_name) {
            self.session_ctx.register_table(table_name, provider)?;
            self.registered_tables.insert(table_name.to_string());
        }

        Ok(())
    }

    pub(crate) fn collect_parquet_files_batches(
        &mut self,
        table_name: &str,
        file_paths: Vec<String>,
        sql_query: Option<&str>,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let batch_stream = self.parquet_files_batch_stream(table_name, file_paths, sql_query)?;
        let schema = batch_stream.schema();
        let mut batches = block_on_nautilus_with(|| batch_stream.try_collect::<Vec<_>>())?;
        if batches.is_empty() {
            batches.push(RecordBatch::new_empty(schema));
        }

        Ok(batches)
    }

    pub(crate) fn parquet_files_batch_stream(
        &mut self,
        table_name: &str,
        file_paths: Vec<String>,
        sql_query: Option<&str>,
    ) -> anyhow::Result<SendableRecordBatchStream> {
        if file_paths.is_empty() {
            return Ok(Box::pin(EmptyRecordBatchStream::new(Arc::new(
                Schema::empty(),
            ))));
        }

        self.register_parquet_files_table(table_name, file_paths)?;
        Ok(self.execute_registered(table_name, sql_query)?)
    }

    fn register_parquet_files_table(
        &mut self,
        table_name: &str,
        file_paths: Vec<String>,
    ) -> anyhow::Result<()> {
        if self.registered_tables.contains(table_name) {
            return Ok(());
        }

        let parquet_options = ParquetReadOptions::<'_> {
            skip_metadata: Some(false),
            ..Default::default()
        };

        let dataframe =
            block_on_nautilus_with(|| self.session_ctx.read_parquet(file_paths, parquet_options))?;

        validate_catalog_schema(dataframe.schema().as_arrow())?;
        self.session_ctx
            .register_table(table_name, dataframe.into_view())?;
        self.registered_tables.insert(table_name.to_string());
        Ok(())
    }

    pub(crate) fn execute_registered(
        &self,
        table_name: &str,
        sql_query: Option<&str>,
    ) -> Result<SendableRecordBatchStream> {
        let default_query = format!("SELECT * FROM {table_name} ORDER BY ts_init");
        let sql_query = sql_query.unwrap_or(&default_query);
        let query = block_on_nautilus_with(|| self.session_ctx.sql(sql_query))?;
        block_on_nautilus_with(|| query.execute_stream())
    }

    /// Clears all registered tables.
    ///
    /// This is useful when the underlying files have changed and we need to
    /// re-register tables with updated data.
    pub fn clear_registered_tables(&mut self) {
        self.registered_tables.clear();

        // Create a new session context to completely reset the DataFusion state
        self.session_ctx = SessionContext::new_with_config(session_config());
    }
}

pub(crate) fn session_config() -> SessionConfig {
    SessionConfig::new()
        .set_str("datafusion.optimizer.repartition_file_scans", "false")
        .set_str("datafusion.optimizer.prefer_existing_sort", "true")
}

pub(crate) fn cast_record_batch_to_schema(
    batch: &RecordBatch,
    schema: &Schema,
) -> Result<RecordBatch> {
    let batch_schema = batch.schema();
    let mut fields = Vec::with_capacity(batch_schema.fields().len());
    let mut columns = Vec::with_capacity(batch.columns().len());

    for (column, batch_field) in batch.columns().iter().zip(batch_schema.fields()) {
        let field = schema
            .field_with_name(batch_field.name())
            .unwrap_or(batch_field.as_ref());

        fields.push(Arc::new(field.clone()));
        columns.push(cast_column_to_data_type(column, field.data_type())?);
    }

    let schema = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn cast_column_to_data_type(column: &ArrayRef, data_type: &DataType) -> Result<ArrayRef> {
    if column.data_type() == data_type {
        return Ok(column.clone());
    }

    if let (DataType::BinaryView, DataType::FixedSizeBinary(width)) =
        (column.data_type(), data_type)
    {
        let array = column
            .as_any()
            .downcast_ref::<BinaryViewArray>()
            .expect("BinaryView column should downcast to BinaryViewArray");
        let mut builder = FixedSizeBinaryBuilder::with_capacity(array.len(), *width);
        for row in 0..array.len() {
            if array.is_null(row) {
                builder.append_null();
            } else {
                builder.append_value(array.value(row))?;
            }
        }

        return Ok(Arc::new(builder.finish()));
    }

    if let (DataType::FixedSizeList(_, size), DataType::List(field)) =
        (column.data_type(), data_type)
    {
        let array = column
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .expect("FixedSizeList column should downcast to FixedSizeListArray");

        let size = usize::try_from(*size).map_err(|e| {
            DataFusionError::Execution(format!("Invalid fixed-size list length {size}: {e}"))
        })?;

        let mut parts = Vec::with_capacity(array.len());
        let mut offsets = Vec::with_capacity(array.len() + 1);
        offsets.push(0_i32);

        for row in 0..array.len() {
            if !array.is_null(row) {
                let value = array.value(row);
                parts.push(cast_column_to_data_type(&value, field.data_type())?);
            }

            let offset = parts
                .len()
                .checked_mul(size)
                .and_then(|offset| i32::try_from(offset).ok())
                .ok_or_else(|| {
                    DataFusionError::Execution(
                        "List offset exceeds the supported i32 range".to_string(),
                    )
                })?;

            offsets.push(offset);
        }

        let values = concat_list_values(&parts, field.data_type())?;
        let offsets = OffsetBuffer::new(ScalarBuffer::from(offsets));
        return Ok(Arc::new(ListArray::try_new(
            field.clone(),
            offsets,
            values,
            array.nulls().cloned(),
        )?));
    }

    if let (DataType::List(_), DataType::FixedSizeList(field, size)) =
        (column.data_type(), data_type)
    {
        let array = column
            .as_any()
            .downcast_ref::<ListArray>()
            .expect("List column should downcast to ListArray");

        let size_usize = usize::try_from(*size).map_err(|e| {
            DataFusionError::Execution(format!("Invalid fixed-size list length {size}: {e}"))
        })?;

        let mut parts = Vec::with_capacity(array.len());
        for row in 0..array.len() {
            if array.is_null(row) {
                parts.push(new_null_array(field.data_type(), size_usize));
                continue;
            }

            let value = array.value(row);
            if value.len() != size_usize {
                return Err(DataFusionError::Execution(format!(
                    "List row {row} has length {}, expected {size}",
                    value.len(),
                )));
            }

            parts.push(cast_column_to_data_type(&value, field.data_type())?);
        }

        let values = concat_list_values(&parts, field.data_type())?;
        return Ok(Arc::new(FixedSizeListArray::try_new(
            field.clone(),
            *size,
            values,
            array.nulls().cloned(),
        )?));
    }

    Ok(cast(column, data_type)?)
}

fn concat_list_values(parts: &[ArrayRef], data_type: &DataType) -> Result<ArrayRef> {
    if parts.is_empty() {
        return Ok(new_empty_array(data_type));
    }

    let parts = parts.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    Ok(concat(&parts)?)
}

#[must_use]
pub fn build_query(
    table: &str,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let filter = where_sql(start, end, where_clause);
    format!("SELECT * FROM {table}{filter} ORDER BY ts_init")
}

#[must_use]
pub fn build_identifier_query(
    table: &str,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let filter = where_sql(start, end, where_clause);
    format!("SELECT DISTINCT identifier FROM {table}{filter} ORDER BY identifier")
}

fn where_sql(
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let mut conditions = Vec::new();

    if let Some(clause) = where_clause {
        // Parenthesized so a caller clause containing a top-level `OR` still binds as one
        // condition once the timestamp bounds are appended with `AND`.
        conditions.push(format!("({clause})"));
    }

    if let Some(start_ts) = start {
        conditions.push(format!("CAST(ts_init AS BIGINT) >= {start_ts}"));
    }

    if let Some(end_ts) = end {
        conditions.push(format!("CAST(ts_init AS BIGINT) <= {end_ts}"));
    }

    if conditions.is_empty() {
        return String::new();
    }

    format!(" WHERE {}", conditions.join(" AND "))
}

pub fn identifiers_from_record_batches(batches: &[RecordBatch]) -> anyhow::Result<Vec<String>> {
    let mut identifiers = AHashSet::new();

    for batch in batches {
        let column = batch
            .column_by_name("identifier")
            .ok_or_else(|| anyhow::anyhow!("identifier column not found"))?
            .as_any();

        if let Some(array) = column.downcast_ref::<StringArray>() {
            for row in 0..array.len() {
                if !array.is_null(row) {
                    identifiers.insert(array.value(row).to_string());
                }
            }

            continue;
        }

        if let Some(array) = column.downcast_ref::<StringViewArray>() {
            for row in 0..array.len() {
                if !array.is_null(row) {
                    identifiers.insert(array.value(row).to_string());
                }
            }

            continue;
        }

        anyhow::bail!("identifier column must be Utf8 or Utf8View");
    }

    let mut identifiers = identifiers.into_iter().collect::<Vec<_>>();
    identifiers.sort();
    Ok(identifiers)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    use datafusion::{
        arrow::{
            array::{Decimal128Array, FixedSizeBinaryArray, UInt32Array},
            buffer::NullBuffer,
            datatypes::Field,
        },
        datasource::MemTable,
        execution::object_store::ObjectStoreUrl,
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::common::storage::create_storage_backend_from_path;

    #[rstest]
    fn register_storage_backend_accepts_memory_backend() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        let mut session = DataBackendSession::new(10);

        session.register_storage_backend(&storage).unwrap();

        assert_registered_object_store(&session, &storage);
    }

    #[rstest]
    fn register_storage_backend_accepts_local_backend() {
        let temp_dir = TempDir::new().unwrap();
        let storage =
            create_storage_backend_from_path(temp_dir.path().to_str().unwrap(), None).unwrap();
        let mut session = DataBackendSession::new(10);

        session.register_storage_backend(&storage).unwrap();

        assert_registered_object_store(&session, &storage);
    }

    fn assert_registered_object_store(session: &DataBackendSession, storage: &StorageBackend) {
        let root_url = ObjectStoreUrl::parse(storage.datafusion_root_url().unwrap()).unwrap();
        let registered = session
            .session_ctx
            .runtime_env()
            .object_store(root_url)
            .unwrap();

        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&registered),
            Arc::as_ptr(&storage.object_store),
        ));
    }

    fn memory_table(column: &str) -> Arc<dyn TableProvider> {
        let schema = Arc::new(Schema::new(vec![Field::new(column, DataType::Utf8, true)]));
        Arc::new(MemTable::try_new(schema, vec![Vec::new()]).unwrap())
    }

    #[rstest]
    fn register_table_provider_keeps_first_registration() {
        let mut session = DataBackendSession::new(10);

        session
            .register_table_provider("records", memory_table("first"))
            .unwrap();
        session
            .register_table_provider("records", memory_table("second"))
            .unwrap();
        let provider =
            futures::executor::block_on(session.session_ctx.table_provider("records")).unwrap();

        assert_eq!(provider.schema().field(0).name(), "first");
    }

    #[rstest]
    fn session_config_keeps_file_scan_order() {
        let config = session_config();
        let optimizer = &config.options().optimizer;

        assert!(!optimizer.repartition_file_scans);
        assert!(optimizer.prefer_existing_sort);
    }

    #[rstest]
    fn identifiers_from_record_batches_reads_utf8_view_columns() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "identifier",
            DataType::Utf8View,
            true,
        )]));
        let batch = RecordBatch::try_new(
            schema,
            vec![Arc::new(StringViewArray::from(vec![
                Some("ESZ4.XCME"),
                None,
                Some("ESM4.XCME"),
            ])) as ArrayRef],
        )
        .unwrap();

        let identifiers = identifiers_from_record_batches(&[batch]).unwrap();

        assert_eq!(identifiers, vec!["ESM4.XCME", "ESZ4.XCME"]);
    }

    #[rstest]
    #[case::missing_column("instrument_id", DataType::Utf8, "identifier column not found")]
    #[case::unsupported_type(
        "identifier",
        DataType::Int32,
        "identifier column must be Utf8 or Utf8View"
    )]
    fn identifiers_from_record_batches_rejects_invalid_columns(
        #[case] name: &str,
        #[case] data_type: DataType,
        #[case] expected: &str,
    ) {
        let column = new_null_array(&data_type, 1);
        let schema = Arc::new(Schema::new(vec![Field::new(name, data_type, true)]));
        let batch = RecordBatch::try_new(schema, vec![column]).unwrap();

        let error = identifiers_from_record_batches(&[batch]).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn binary_view_cast_to_fixed_size_binary_preserves_nulls() {
        let values = vec![Some(b"ab".as_slice()), None, Some(b"cd".as_slice())];
        let column = Arc::new(BinaryViewArray::from(values.clone())) as ArrayRef;

        let cast = cast_column_to_data_type(&column, &DataType::FixedSizeBinary(2)).unwrap();

        let expected =
            FixedSizeBinaryArray::try_from_sparse_iter_with_size(values.into_iter(), 2).unwrap();
        assert_eq!(cast.to_data(), expected.to_data());
    }

    #[rstest]
    fn list_cast_to_fixed_size_list_rejects_wrong_row_length() {
        let field = Arc::new(Field::new("element", DataType::UInt32, false));
        let list = Arc::new(
            ListArray::try_new(
                field.clone(),
                OffsetBuffer::new(ScalarBuffer::from(vec![0_i32, 3])),
                Arc::new(UInt32Array::from(vec![1_u32, 2, 3])),
                None,
            )
            .unwrap(),
        ) as ArrayRef;

        let error =
            cast_column_to_data_type(&list, &DataType::FixedSizeList(field, 2)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Execution error: List row 0 has length 3, expected 2"
        );
    }

    struct DropSignal(mpsc::Sender<()>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[rstest]
    fn blocking_batch_stream_drop_stops_producer() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (dropped_sender, dropped_receiver) = mpsc::channel();
        let signal = DropSignal(dropped_sender);

        let stream = futures::stream::pending::<i32>().chain(futures::stream::once(async move {
            drop(signal);
            0
        }));

        let stream = BlockingBatchStream::from_stream_with_runtime(stream, runtime.handle());

        drop(stream);

        assert_eq!(
            dropped_receiver.recv_timeout(Duration::from_secs(5)),
            Ok(())
        );
    }

    #[rstest]
    fn build_identifier_query_projects_distinct_identifier_with_filters() {
        let query = build_identifier_query(
            "quotes",
            Some(UnixNanos::from(10)),
            Some(UnixNanos::from(20)),
            Some("instrument_id LIKE 'ES%'"),
        );

        assert_eq!(
            query,
            "SELECT DISTINCT identifier FROM quotes WHERE (instrument_id LIKE 'ES%') \
             AND CAST(ts_init AS BIGINT) >= 10 AND CAST(ts_init AS BIGINT) <= 20 ORDER BY identifier"
        );
    }

    #[rstest]
    fn build_query_groups_a_disjunctive_where_clause_against_timestamp_bounds() {
        let query = build_query(
            "bars",
            Some(UnixNanos::from(10)),
            Some(UnixNanos::from(20)),
            Some("identifier = 'ES.GLBX' OR starts_with(identifier, 'ES.GLBX-')"),
        );

        assert_eq!(
            query,
            "SELECT * FROM bars WHERE (identifier = 'ES.GLBX' OR starts_with(identifier, 'ES.GLBX-')) \
             AND CAST(ts_init AS BIGINT) >= 10 AND CAST(ts_init AS BIGINT) <= 20 ORDER BY ts_init"
        );
    }

    #[rstest]
    fn build_query_without_bounds_keeps_the_where_clause_alone() {
        let query = build_query("bars", None, None, Some("identifier = 'ES.GLBX'"));

        assert_eq!(
            query,
            "SELECT * FROM bars WHERE (identifier = 'ES.GLBX') ORDER BY ts_init"
        );
    }

    #[rstest]
    fn build_query_without_conditions_omits_the_where_keyword() {
        let query = build_query("bars", None, None, None);

        assert_eq!(query, "SELECT * FROM bars ORDER BY ts_init");
    }

    #[rstest]
    fn identifiers_from_record_batches_returns_sorted_unique_non_null_values() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "identifier",
            DataType::Utf8,
            true,
        )]));
        let batch = RecordBatch::try_new(
            schema,
            vec![Arc::new(StringArray::from(vec![
                Some("ESZ4.XCME"),
                None,
                Some("ESM4.XCME"),
                Some("ESZ4.XCME"),
            ])) as ArrayRef],
        )
        .unwrap();

        let identifiers = identifiers_from_record_batches(&[batch]).unwrap();

        assert_eq!(identifiers, vec!["ESM4.XCME", "ESZ4.XCME"]);
    }

    #[rstest]
    fn fixed_size_list_cast_round_trips_through_delta_list_type() {
        let field = Arc::new(Field::new("element", DataType::Decimal128(38, 16), true));
        let values = Decimal128Array::from(vec![1_i128, 2, 3, 4])
            .with_precision_and_scale(38, 16)
            .unwrap();
        let fixed_type = DataType::FixedSizeList(field.clone(), 2);
        let fixed = Arc::new(
            FixedSizeListArray::try_new(field.clone(), 2, Arc::new(values), None).unwrap(),
        ) as ArrayRef;

        let list_type = DataType::List(field);
        let list = cast_column_to_data_type(&fixed, &list_type).unwrap();
        let restored = cast_column_to_data_type(&list, &fixed_type).unwrap();

        assert_eq!(list.data_type(), &list_type);
        assert_eq!(restored.data_type(), &fixed_type);
        assert_eq!(restored.to_data(), fixed.to_data());

        let field = Arc::new(Field::new("element", DataType::UInt32, false));
        let fixed = Arc::new(
            FixedSizeListArray::try_new(
                field.clone(),
                2,
                Arc::new(UInt32Array::from(vec![1_u32, 2])),
                Some(NullBuffer::from(vec![false])),
            )
            .unwrap(),
        ) as ArrayRef;
        let list_type = DataType::List(field);
        let list = cast_column_to_data_type(&fixed, &list_type).unwrap();
        let list = list.as_any().downcast_ref::<ListArray>().unwrap();

        assert!(list.is_null(0));
        assert_eq!(list.values().len(), 0);
    }

    #[rstest]
    fn data_backend_sessions_share_global_runtime() {
        let first = DataBackendSession::new(10);
        let second = DataBackendSession::new(10);

        assert_eq!(first.runtime.id(), second.runtime.id());
        assert_eq!(first.runtime.id(), get_runtime().handle().id());
    }

    #[rstest]
    fn blocking_batch_stream_prefetches_first_item() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (polled_sender, polled_receiver) = mpsc::channel();

        let stream = futures::stream::once(async move {
            polled_sender.send(()).unwrap();
            42
        });

        let mut stream = BlockingBatchStream::from_stream_with_runtime(stream, runtime.handle());

        assert_eq!(polled_receiver.recv_timeout(Duration::from_secs(1)), Ok(()),);
        assert_eq!(stream.next(), Some(42));
    }

    #[rstest]
    fn blocking_batch_stream_works_inside_current_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            let mut stream = BlockingBatchStream::from_stream_with_runtime(
                futures::stream::iter([42]),
                get_runtime().handle(),
            );

            assert_eq!(stream.next(), Some(42));
        });
    }
}
