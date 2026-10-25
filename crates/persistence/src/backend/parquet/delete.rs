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

//! Range-based deletion and split operations for the parquet catalog.

use std::{collections::BTreeMap, sync::Arc};

use ahash::AHashSet;
use arrow::{
    array::{ArrayRef, StringArray},
    compute::cast,
    datatypes::DataType as ArrowDataType,
    record_batch::RecordBatch,
    row::{RowConverter, SortField},
};
use nautilus_core::{UnixNanos, string::conversions::to_snake_case};
use nautilus_model::data::{
    Bar, CustomData, Data, DataType, FundingRateUpdate, HasTsInit, IndexPriceUpdate,
    InstrumentStatus, MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta,
    OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
};
use nautilus_serialization::arrow::{
    DecodeTypedFromRecordBatch, EncodeToRecordBatch, KEY_BAR_TYPE, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, KEY_TYPE_NAME, is_json_string_field,
};

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        io::read_parquet_from_object_store,
        paths::{make_object_store_path, parse_filename_timestamps, timestamps_to_filename},
    },
    catalog::types::{
        HasCatalogDataType, data_type_from_data_path_prefix, parquet_data_path_prefix,
    },
};

/// Kind of deletion operation to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOperationKind {
    /// Remove the files entirely.
    Remove,
    /// Rewrite the data preserved before the deleted range, then remove the files.
    SplitBefore,
    /// Rewrite the data preserved after the deleted range, then remove the files.
    SplitAfter,
}

/// Information about a deletion operation to be executed.
#[derive(Debug, Clone)]
pub struct DeleteOperation {
    /// Kind of deletion operation.
    pub kind: DeleteOperationKind,
    /// List of files involved in this operation.
    pub files: Vec<String>,
    /// Start timestamp for data query (used for split operations).
    pub query_start: u64,
    /// End timestamp for data query (used for split operations).
    pub query_end: u64,
    /// Start timestamp for new file naming (used for split operations).
    pub file_start_ns: u64,
    /// End timestamp for new file naming (used for split operations).
    pub file_end_ns: u64,
}

impl ParquetDataCatalog {
    /// Deletes data within a specified time range across the entire catalog.
    ///
    /// This method identifies all leaf directories in the catalog that contain parquet files
    /// and deletes data within the specified time range from each directory. A leaf directory
    /// is one that contains files but no subdirectories. This is a convenience method that
    /// effectively calls `delete_data_range` for all data types and instrument IDs in the catalog.
    ///
    /// # Parameters
    ///
    /// - `start`: Optional start timestamp for the deletion range. If None, deletes from the beginning.
    /// - `end`: Optional end timestamp for the deletion range. If None, deletes to the end.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if deletion fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory traversal fails.
    /// - Data class extraction from paths fails.
    /// - Individual delete operations fail.
    ///
    /// # Notes
    ///
    /// - This operation permanently removes data and cannot be undone.
    /// - The deletion process handles file intersections intelligently by splitting files
    ///   when they partially overlap with the deletion range.
    /// - Files completely within the deletion range are removed entirely.
    /// - Files partially overlapping the deletion range are split to preserve data outside the range.
    /// - This method is useful for bulk data cleanup operations across the entire catalog.
    /// - Empty directories are not automatically removed after deletion.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_core::UnixNanos;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Delete all data before a specific date across entire catalog
    /// catalog.delete_catalog_range(None, Some(UnixNanos::from(1609459200000000000)))?;
    ///
    /// // Delete all data within a specific range across entire catalog
    /// catalog.delete_catalog_range(
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    /// )?;
    ///
    /// // Delete all data after a specific date across entire catalog
    /// catalog.delete_catalog_range(Some(UnixNanos::from(1609459200000000000)), None)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn delete_catalog_range(
        &mut self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            let Ok((Some(data_cls), identifier)) =
                self.extract_data_cls_and_identifier_from_path(&directory)
            else {
                continue;
            };

            let Ok(data_type) = data_type_from_data_path_prefix(&data_cls) else {
                log::warn!("Skipping directory {directory}: unknown data class {data_cls}");
                continue;
            };

            self.delete_data_range(&data_type, identifier.as_deref(), start, end)?;
        }

        Ok(())
    }

    /// Deletes data within a specified time range for a specific data type and identifier.
    ///
    /// This method identifies all parquet files that intersect with the specified time range
    /// and handles them appropriately:
    /// - Files completely within the range are deleted
    /// - Files partially overlapping the range are split to preserve data outside the range
    /// - The original intersecting files are removed after processing
    ///
    /// # Parameters
    ///
    /// - `data_type`: The data type to delete from.
    /// - `identifier`: Optional identifier to delete data for. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If None, deletes data across all identifiers.
    /// - `start`: Optional start timestamp for the deletion range. If None, deletes from the beginning.
    /// - `end`: Optional end timestamp for the deletion range. If None, deletes to the end.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if deletion fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `start` exceeds `end` and the directory holds data to delete.
    /// - The directory path cannot be constructed.
    /// - File operations fail.
    /// - Data querying or writing fails.
    ///
    /// # Notes
    ///
    /// - This operation permanently removes data and cannot be undone.
    /// - Files that partially overlap the deletion range are split to preserve data outside the range.
    /// - The method ensures data integrity by using atomic operations where possible.
    /// - Empty directories are not automatically removed after deletion.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_core::UnixNanos;
    /// use nautilus_model::data::NautilusDataType;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Delete all quote data for a specific instrument
    /// catalog.delete_data_range(&NautilusDataType::QuoteTick, Some("BTCUSD"), None, None)?;
    ///
    /// // Delete trade data within a specific time range
    /// catalog.delete_data_range(
    ///     &NautilusDataType::TradeTick,
    ///     None,
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn delete_data_range(
        &mut self,
        data_type: &NautilusDataType,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()> {
        match data_type {
            NautilusDataType::QuoteTick => {
                self.delete_data_range_generic::<QuoteTick>(identifier, start, end)
            }
            NautilusDataType::TradeTick => {
                self.delete_data_range_generic::<TradeTick>(identifier, start, end)
            }
            NautilusDataType::Bar => self.delete_data_range_generic::<Bar>(identifier, start, end),
            NautilusDataType::OrderBookDelta => {
                self.delete_data_range_generic::<OrderBookDelta>(identifier, start, end)
            }
            NautilusDataType::OrderBookDepth => {
                self.delete_data_range_generic::<OrderBookDepth>(identifier, start, end)
            }
            NautilusDataType::MarkPriceUpdate => {
                self.delete_data_range_generic::<MarkPriceUpdate>(identifier, start, end)
            }
            NautilusDataType::IndexPriceUpdate => {
                self.delete_data_range_generic::<IndexPriceUpdate>(identifier, start, end)
            }
            NautilusDataType::InstrumentClose => {
                self.delete_data_range_generic::<InstrumentClose>(identifier, start, end)
            }
            NautilusDataType::FundingRateUpdate => {
                self.delete_data_range_generic::<FundingRateUpdate>(identifier, start, end)
            }
            NautilusDataType::OptionGreeks => {
                self.delete_data_range_generic::<OptionGreeks>(identifier, start, end)
            }
            NautilusDataType::InstrumentStatus => {
                self.delete_data_range_generic::<InstrumentStatus>(identifier, start, end)
            }
            NautilusDataType::Custom { type_name } => {
                self.delete_custom_data_range(type_name, identifier, start, end)
            }
            other @ NautilusDataType::Instrument => {
                anyhow::bail!("Unsupported data type: {other}")
            }
            #[cfg(feature = "defi")]
            other @ NautilusDataType::Defi => {
                anyhow::bail!("Unsupported data type: {other}")
            }
            #[cfg(not(feature = "defi"))]
            #[allow(
                unreachable_patterns,
                reason = "DeFi variants can exist without this crate's defi feature"
            )]
            other => anyhow::bail!("Unsupported data type: {other}"),
        }
    }

    /// Generic implementation for deleting data within a specified time range.
    ///
    /// This method provides the core deletion logic that works with any data type
    /// that implements the required traits. It handles file intersection analysis,
    /// data splitting for partial overlaps, and file cleanup.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type that implements required traits for catalog operations.
    ///
    /// # Parameters
    ///
    /// - `identifier`: Optional instrument ID to delete data for.
    /// - `start`: Optional start timestamp for the deletion range.
    /// - `end`: Optional end timestamp for the deletion range.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if deletion fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `start` exceeds `end` and the directory holds data to delete.
    /// - Data querying, encoding, or file operations fail.
    pub fn delete_data_range_generic<T>(
        &mut self,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()>
    where
        T: DecodeTypedFromRecordBatch
            + HasCatalogDataType
            + EncodeToRecordBatch
            + HasTsInit
            + TryFrom<Data>
            + Clone,
    {
        // Get intervals for cleaner implementation
        let data_type = T::catalog_data_type();
        let files = self.files_for_delete(&data_type, identifier)?;

        if files.is_empty() {
            return Ok(());
        }

        let operations_to_execute = operations_for_files(&files, start, end)?;

        if operations_to_execute.is_empty() {
            return Ok(()); // No operations to execute
        }

        let file_paths: AHashSet<&str> = files.iter().map(|(path, ..)| path.as_str()).collect();
        let (targets, files_to_remove) = group_operations_by_target(operations_to_execute);
        let type_name = to_snake_case(std::any::type_name::<T>());
        let mut writes = Vec::new();

        // Splits of overlapping files can target the same file, so each target is written once
        // with the rows of all its splits; writing them in turn would skip all but the first.
        // Every target is checked before any file is written or removed. Each pass holds the
        // rows of one target only, so the write pass queries them again.
        for (target, sources) in &targets {
            let rows = self.query_split_rows::<T>(identifier, target, sources)?;

            if rows.is_empty() {
                continue;
            }

            Self::check_chunk_identity(&rows, &type_name)?;
            if file_paths.contains(target.0.as_str()) {
                let batches = self.data_to_record_batches(&rows)?;
                self.ensure_split_target_holds_rows(&target.0, &batches)?;
                continue;
            }

            writes.push((target, sources));
        }

        for (target, sources) in writes {
            let rows = self.query_split_rows::<T>(identifier, target, sources)?;
            self.write_to_parquet(
                &rows,
                Some(UnixNanos::from(target.1)),
                Some(UnixNanos::from(target.2)),
                Some(true),
            )?;
        }

        for file in files_to_remove {
            self.delete_file(&file)?;
        }

        Ok(())
    }

    /// Deletes custom data within a specified time range.
    ///
    /// This method provides deletion for custom data types that don't have compile-time
    /// type information. It uses dynamic querying and writing methods.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The custom data type name (without "custom/" prefix).
    /// - `identifier`: Optional instrument ID to delete data for.
    /// - `start`: Optional start timestamp for the deletion range.
    /// - `end`: Optional end timestamp for the deletion range.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if deletion fails.
    fn delete_custom_data_range(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()> {
        let data_type = NautilusDataType::Custom {
            type_name: type_name.to_string(),
        };

        let files = self.files_for_delete(&data_type, identifier)?;

        if files.is_empty() {
            return Ok(());
        }

        let operations_to_execute = operations_for_files(&files, start, end)?;

        if operations_to_execute.is_empty() {
            return Ok(()); // No operations to execute
        }

        let file_paths: AHashSet<&str> = files.iter().map(|(path, ..)| path.as_str()).collect();
        let (targets, files_to_remove) = group_operations_by_target(operations_to_execute);
        let mut writes = Vec::new();

        // Splits of overlapping files can target the same file, so each target is written once
        // with the rows of all its splits. Every target and group is checked before any file is
        // written or removed, so groups with different metadata that would share a file fail the
        // delete without changes. Each pass holds the rows of one target only, so the write pass
        // queries them again.
        for (target, sources) in &targets {
            let rows = self.query_custom_split_rows(type_name, identifier, target, sources)?;

            if rows.is_empty() {
                continue;
            }

            let target_writes = self.prepare_custom_data_groups(
                &rows,
                Some(UnixNanos::from(target.1)),
                Some(UnixNanos::from(target.2)),
                Some(true),
            )?;

            if file_paths.contains(target.0.as_str()) {
                let batches: Vec<RecordBatch> = target_writes
                    .into_iter()
                    .flat_map(|write| write.batches)
                    .collect();
                self.ensure_split_target_holds_rows(&target.0, &batches)?;
                continue;
            }

            writes.push((target, sources));
        }

        for (target, sources) in writes {
            let rows = self.query_custom_split_rows(type_name, identifier, target, sources)?;
            self.write_custom_data_groups(
                &rows,
                Some(UnixNanos::from(target.1)),
                Some(UnixNanos::from(target.2)),
                Some(true),
            )?;
        }

        for file in files_to_remove {
            self.delete_file(&file)?;
        }

        Ok(())
    }

    // Queries the rows a split target preserves from its source files, sorted by `ts_init`.
    fn query_split_rows<T>(
        &mut self,
        identifier: Option<&str>,
        target: &SplitTarget,
        sources: &[String],
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeTypedFromRecordBatch + HasCatalogDataType + HasTsInit,
    {
        // Reset the session so each query loads the source files afresh
        self.clear_session_tables();
        // Use optimize_file_loading=false for precise file control during split operations
        let instrument_ids = identifier.map(|id| vec![id.to_string()]);
        let mut rows = self.query::<T>(
            instrument_ids,
            Some(UnixNanos::from(target.1)),
            Some(UnixNanos::from(target.2)),
            None,
            Some(sources.to_vec()),
            false,
        )?;
        rows.sort_by_key(HasTsInit::ts_init);
        Ok(rows)
    }

    // Queries the custom rows a split target preserves from its source files, sorted by
    // `ts_init`.
    fn query_custom_split_rows(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        target: &SplitTarget,
        sources: &[String],
    ) -> anyhow::Result<Vec<CustomData>> {
        self.clear_session_tables();
        let instrument_ids = identifier.map(|id| vec![id.to_string()]);
        let mut rows: Vec<CustomData> = self
            .query_custom_data_dynamic(
                type_name,
                instrument_ids.as_deref(),
                Some(UnixNanos::from(target.1)),
                Some(UnixNanos::from(target.2)),
                None,
                Some(sources.to_vec()),
                false,
            )?
            .into_iter()
            .filter_map(|data| match data {
                Data::Custom(c) => Some(c),
                _ => None,
            })
            .collect();
        rows.sort_by_key(|custom| custom.data.ts_init());
        Ok(rows)
    }

    // An existing split target is skipped by the write, and the split's sources are then removed,
    // so it must already hold every preserved row, as after a rerun of an interrupted delete.
    // Rows are compared in their Arrow row encoding, where equal values match, NaN included,
    // unlike `PartialEq` on decoded rows.
    fn ensure_split_target_holds_rows(
        &self,
        target: &str,
        rows: &[RecordBatch],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.target_holds_rows(target, rows)?,
            "Cannot preserve the rows outside the deleted range: their target file {target} \
             already exists without them"
        );
        Ok(())
    }

    pub(crate) fn target_holds_rows(
        &self,
        target: &str,
        rows: &[RecordBatch],
    ) -> anyhow::Result<bool> {
        let object_path = self.to_object_path_parsed(target)?;

        let (existing, schema) = self.execute_async(|| async {
            read_parquet_from_object_store(self.object_store.clone(), &object_path).await
        })?;

        for batch in rows {
            for key in [
                KEY_INSTRUMENT_ID,
                KEY_BAR_TYPE,
                KEY_PRICE_PRECISION,
                KEY_SIZE_PRECISION,
                KEY_TYPE_NAME,
            ] {
                if schema.metadata().get(key) != batch.schema().metadata().get(key) {
                    return Ok(false);
                }
            }
        }

        batches_hold_rows(&existing, rows)
    }

    fn files_for_delete(
        &self,
        data_type: &NautilusDataType,
        identifier: Option<&str>,
    ) -> anyhow::Result<Vec<(String, u64, u64)>> {
        let directory = if let NautilusDataType::Custom { type_name } = data_type {
            self.make_path_custom_data(type_name, identifier)?
        } else {
            let prefix = parquet_data_path_prefix(data_type);
            self.make_path(prefix.as_ref(), identifier)?
        };

        let files = self.list_parquet_files(&directory)?;

        Ok(files
            .into_iter()
            .filter_map(|path| {
                parse_filename_timestamps(&path).map(|(start, end)| (path, start, end))
            })
            .collect())
    }

    /// Prepares all operations for data deletion by identifying files that need to be
    /// split or removed.
    ///
    /// This auxiliary function handles all the preparation logic for deletion:
    /// 1. Filters intervals by time range
    /// 2. Identifies files that intersect with the deletion range
    /// 3. Creates split operations for files that partially overlap
    /// 4. Generates removal operations for files completely within the range
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name for path generation.
    /// - `identifier`: Optional instrument identifier for path generation.
    /// - `intervals`: List of (`start_ts`, `end_ts`) tuples representing existing file intervals.
    /// - `start`: Optional start timestamp for deletion range.
    /// - `end`: Optional end timestamp for deletion range.
    ///
    /// # Returns
    ///
    /// Returns a vector of `DeleteOperation` structs ready for execution.
    ///
    /// Plans from the supplied intervals. It does not look up stored filenames, so a
    /// replay-identity suffix is not included. Catalog deletion resolves stored paths itself.
    ///
    /// # Errors
    ///
    /// Returns an error if `start` exceeds `end` or the directory path cannot be constructed.
    pub fn prepare_delete_operations(
        &self,
        type_name: &str,
        identifier: Option<&str>,
        intervals: &[(u64, u64)],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<DeleteOperation>> {
        let directory = self.make_path(type_name, identifier)?;

        let files = intervals
            .iter()
            .map(|&(file_start_ns, file_end_ns)| {
                let filename = timestamps_to_filename(
                    UnixNanos::from(file_start_ns),
                    UnixNanos::from(file_end_ns),
                );
                let file_path = make_object_store_path(&directory, [&filename]);
                (file_path, file_start_ns, file_end_ns)
            })
            .collect::<Vec<_>>();

        operations_for_files(&files, start, end)
    }
}

fn operations_for_files(
    files: &[(String, u64, u64)],
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> anyhow::Result<Vec<DeleteOperation>> {
    if let (Some(start), Some(end)) = (start, end) {
        anyhow::ensure!(start <= end, "deletion start must not exceed end");
    }

    let delete_start_ns = start.map(|s| s.as_u64());
    let delete_end_ns = end.map(|e| e.as_u64());
    let mut operations = Vec::new();

    for (file_path, file_start_ns, file_end_ns) in files {
        let intersects = delete_start_ns.is_none_or(|start| start <= *file_end_ns)
            && delete_end_ns.is_none_or(|end| *file_start_ns <= end);

        if !intersects {
            continue;
        }

        let file_completely_within_range = delete_start_ns
            .is_none_or(|start| start <= *file_start_ns)
            && delete_end_ns.is_none_or(|end| *file_end_ns <= end);

        if file_completely_within_range {
            operations.push(DeleteOperation {
                kind: DeleteOperationKind::Remove,
                files: vec![file_path.clone()],
                query_start: 0,
                query_end: 0,
                file_start_ns: 0,
                file_end_ns: 0,
            });

            continue;
        }

        if let Some(delete_start) = delete_start_ns
            && *file_start_ns < delete_start
        {
            operations.push(DeleteOperation {
                kind: DeleteOperationKind::SplitBefore,
                files: vec![file_path.clone()],
                query_start: *file_start_ns,
                query_end: delete_start.saturating_sub(1),
                file_start_ns: *file_start_ns,
                file_end_ns: delete_start.saturating_sub(1),
            });
        }

        if let Some(delete_end) = delete_end_ns
            && delete_end < *file_end_ns
        {
            operations.push(DeleteOperation {
                kind: DeleteOperationKind::SplitAfter,
                files: vec![file_path.clone()],
                query_start: delete_end.saturating_add(1),
                query_end: *file_end_ns,
                file_start_ns: delete_end.saturating_add(1),
                file_end_ns: *file_end_ns,
            });
        }
    }

    Ok(operations)
}

// A file a split operation writes: its source directory and file name, with the file range.
type SplitTarget = (String, u64, u64);

// Groups split operations by the file they write, with the source files of each, and collects
// every file the delete removes. All splits of one target share its range as their query range.
fn group_operations_by_target(
    operations: Vec<DeleteOperation>,
) -> (BTreeMap<SplitTarget, Vec<String>>, AHashSet<String>) {
    let mut targets = BTreeMap::<SplitTarget, Vec<String>>::new();
    let mut files_to_remove = AHashSet::new();

    for operation in operations {
        if operation.kind != DeleteOperationKind::Remove {
            targets
                .entry(split_target(&operation))
                .or_default()
                .extend(operation.files.iter().cloned());
        }
        files_to_remove.extend(operation.files);
    }

    (targets, files_to_remove)
}

fn split_target(operation: &DeleteOperation) -> SplitTarget {
    let source = &operation.files[0];
    let filename = timestamps_to_filename(
        UnixNanos::from(operation.file_start_ns),
        UnixNanos::from(operation.file_end_ns),
    );
    let target = match source.rsplit_once('/') {
        Some((directory, _)) => make_object_store_path(directory, [&filename]),
        None => filename,
    };

    (target, operation.file_start_ns, operation.file_end_ns)
}

// Returns whether every row of `rows` occurs in `existing`, comparing the columns both share by
// name, so a column only one side carries, such as a stored identifier column, is ignored.
// JSON columns compare by value, since an equal map can serialize its keys in any order: custom
// JSON-backed fields and the plain UTF-8 `data_type` column that holds custom data metadata.
fn batches_hold_rows(existing: &[RecordBatch], rows: &[RecordBatch]) -> anyhow::Result<bool> {
    let Some(first) = rows.first() else {
        return Ok(true);
    };
    let Some(existing_first) = existing.first() else {
        return Ok(rows.iter().all(|batch| batch.num_rows() == 0));
    };

    let existing_schema = existing_first.schema();
    let names: Vec<String> = first
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .filter(|name| existing_schema.field_with_name(name).is_ok())
        .collect();
    let fields = names
        .iter()
        .map(|name| {
            let data_type = first.schema().field_with_name(name)?.data_type().clone();
            Ok(SortField::new(data_type))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let converter = RowConverter::new(fields)?;
    let json_names: AHashSet<&str> = names
        .iter()
        .filter(|name| {
            *name == "data_type"
                || [first.schema(), existing_schema.clone()]
                    .iter()
                    .any(|schema| schema.field_with_name(name).is_ok_and(is_json_string_field))
        })
        .map(String::as_str)
        .collect();
    let columns = |batch: &RecordBatch| {
        names
            .iter()
            .map(|name| {
                let column = batch
                    .column_by_name(name)
                    .ok_or_else(|| anyhow::anyhow!("Missing column {name} in Parquet batch"))?;

                if json_names.contains(name.as_str()) {
                    canonical_json_column(column, name == "data_type")
                } else {
                    Ok(column.clone())
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()
    };

    let mut existing_rows = AHashSet::new();

    for batch in existing {
        for row in &converter.convert_columns(&columns(batch)?)? {
            existing_rows.insert(row.as_ref().to_vec());
        }
    }

    for batch in rows {
        for row in &converter.convert_columns(&columns(batch)?)? {
            if !existing_rows.contains(row.as_ref()) {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

// Rewrites each JSON value with object keys sorted, so equal values compare equal as strings.
fn canonical_json_column(column: &ArrayRef, is_data_type: bool) -> anyhow::Result<ArrayRef> {
    let column = cast(column, &ArrowDataType::Utf8)?;
    let values = column
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("JSON column is not a string array"))?;
    let canonical = values
        .iter()
        .map(|value| {
            value
                .map(|value| {
                    let value: serde_json::Value = if is_data_type {
                        let data_type = DataType::from_persistence_json(value)?;
                        let json = data_type.to_persistence_json()?;
                        serde_json::from_str(&json)?
                    } else {
                        serde_json::from_str(value)?
                    };

                    Ok::<_, anyhow::Error>(canonical_json_value(value).to_string())
                })
                .transpose()
        })
        .collect::<Result<StringArray, _>>()?;
    Ok(Arc::new(canonical))
}

fn canonical_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json_value(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json_value).collect())
        }
        value => value,
    }
}
