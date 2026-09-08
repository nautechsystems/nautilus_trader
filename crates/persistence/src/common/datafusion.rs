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
        datatypes::{DataType, Field, Schema},
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

use crate::common::storage::StorageBackend;

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
        let session_ctx = SessionContext::new_with_config(session_config());
        Self {
            session_ctx,
            chunk_size,
            runtime: get_runtime().handle().clone(),
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
        if !self.registered_tables.contains(table_name) {
            let parquet_options = ParquetReadOptions::<'_> {
                skip_metadata: Some(false),
                ..Default::default()
            };
            let dataframe = block_on_nautilus_with(|| {
                self.session_ctx.read_parquet(file_paths, parquet_options)
            })?;
            self.session_ctx
                .register_table(table_name, dataframe.into_view())?;
            self.registered_tables.insert(table_name.to_string());
        }

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

fn session_config() -> SessionConfig {
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
        columns.push(cast_column_to_field(column, field)?);
    }

    let schema = Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()));
    Ok(RecordBatch::try_new(schema, columns)?)
}

fn cast_column_to_field(column: &ArrayRef, field: &Field) -> Result<ArrayRef> {
    cast_column_to_data_type(column, field.data_type())
}

#[expect(
    clippy::too_many_lines,
    reason = "the function keeps recursive Arrow cast rules in one exhaustive type dispatcher"
)]
pub(crate) fn cast_column_to_data_type(
    column: &ArrayRef,
    data_type: &DataType,
) -> Result<ArrayRef> {
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
        let values = if parts.is_empty() {
            new_empty_array(field.data_type())
        } else {
            let parts = parts
                .iter()
                .map(std::convert::AsRef::as_ref)
                .collect::<Vec<_>>();
            concat(&parts)?
        };
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
        let values = if parts.is_empty() {
            new_empty_array(field.data_type())
        } else {
            let parts = parts
                .iter()
                .map(std::convert::AsRef::as_ref)
                .collect::<Vec<_>>();
            concat(&parts)?
        };
        return Ok(Arc::new(FixedSizeListArray::try_new(
            field.clone(),
            *size,
            values,
            array.nulls().cloned(),
        )?));
    }

    Ok(cast(column, data_type)?)
}

#[must_use]
pub fn build_query(
    table: &str,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let conditions = query_conditions(start, end, where_clause);
    let mut query = format!("SELECT * FROM {table}");

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" ORDER BY ts_init");
    query
}

#[must_use]
pub fn build_identifier_query(
    table: &str,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> String {
    let conditions = query_conditions(start, end, where_clause);
    let mut query = format!("SELECT DISTINCT identifier FROM {table}");

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" ORDER BY identifier");
    query
}

fn query_conditions(
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    where_clause: Option<&str>,
) -> Vec<String> {
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

    conditions
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

    use nautilus_common::live::get_runtime;
    use nautilus_model::{
        data::{DataBatch, QuoteTick},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        catalog::session::{DataBatchQuery, TypedDataBatchSession},
        common::storage::create_storage_backend_from_path,
    };

    fn typed_quote(ts_init: u64) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.1"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )
    }

    fn batch_ts(batch: &DataBatch) -> Vec<u64> {
        match batch {
            DataBatch::Quote(quotes) => quotes
                .as_ref()
                .iter()
                .map(|quote| quote.ts_init.as_u64())
                .collect(),
            other => panic!("expected quote batch, found {other:?}"),
        }
    }

    #[rstest]
    fn typed_session_chunks_pages_with_carry_across_pulls() {
        let pages: Vec<anyhow::Result<Vec<QuoteTick>>> = vec![
            Ok(vec![typed_quote(1), typed_quote(2), typed_quote(3)]),
            Ok(Vec::new()),
            Ok(vec![typed_quote(4), typed_quote(5)]),
        ];
        let mut session = TypedDataBatchSession::new(Box::new(pages.into_iter()), Some(2));

        assert_eq!(batch_ts(&session.next_batch().unwrap().unwrap()), [1, 2]);
        assert_eq!(batch_ts(&session.next_batch().unwrap().unwrap()), [3, 4]);
        assert_eq!(batch_ts(&session.next_batch().unwrap().unwrap()), [5]);
        assert!(session.next_batch().unwrap().is_none());
    }

    #[rstest]
    fn typed_session_extends_chunk_across_equal_boundary_ts() {
        let data = vec![
            typed_quote(1),
            typed_quote(2),
            typed_quote(2),
            typed_quote(2),
            typed_quote(3),
        ];
        let mut session = TypedDataBatchSession::from_vec(data, Some(2));

        assert_eq!(
            batch_ts(&session.next_batch().unwrap().unwrap()),
            [1, 2, 2, 2]
        );
        assert_eq!(batch_ts(&session.next_batch().unwrap().unwrap()), [3]);
        assert!(session.next_batch().unwrap().is_none());
    }

    #[rstest]
    fn typed_session_empty_source_yields_none() {
        let mut session = TypedDataBatchSession::<QuoteTick>::from_vec(Vec::new(), None);

        assert!(session.next_batch().unwrap().is_none());
    }

    #[rstest]
    fn typed_session_propagates_page_error() {
        let pages: Vec<anyhow::Result<Vec<QuoteTick>>> = vec![
            Ok(vec![typed_quote(1)]),
            Err(anyhow::anyhow!("page failed")),
        ];
        let mut session = TypedDataBatchSession::new(Box::new(pages.into_iter()), Some(4));

        assert_eq!(session.next_batch().unwrap_err().to_string(), "page failed");
    }

    #[rstest]
    fn register_storage_backend_accepts_memory_backend() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        let mut session = DataBackendSession::new(10);

        session.register_storage_backend(&storage).unwrap();
    }

    #[rstest]
    fn register_storage_backend_accepts_local_backend() {
        let temp_dir = TempDir::new().unwrap();
        let storage =
            create_storage_backend_from_path(temp_dir.path().to_str().unwrap(), None).unwrap();
        let mut session = DataBackendSession::new(10);

        session.register_storage_backend(&storage).unwrap();
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
        use arrow::{
            array::{Decimal128Array, UInt32Array},
            buffer::NullBuffer,
        };

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
