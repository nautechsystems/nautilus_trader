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

#![expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "delete operations forward catalog/storage errors and operate on validated batches"
)]

use ahash::AHashSet;
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, CustomData, Data, FundingRateUpdate, HasTsInit, IndexPriceUpdate, InstrumentStatus,
    MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick,
    TradeTick, close::InstrumentClose,
};
use nautilus_serialization::arrow::{DecodeTypedFromRecordBatch, EncodeToRecordBatch};

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        paths::{make_object_store_path, timestamps_to_filename},
    },
    catalog::types::{CatalogDataType, parquet_data_path_prefix},
    common::custom::group_custom_data_by_type,
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
        let path_prefix = parquet_data_path_prefix(&NautilusDataType::Custom {
            type_name: type_name.to_string(),
        });

        // Get intervals for the custom data type
        let intervals = self.get_intervals(path_prefix.as_ref(), identifier)?;

        if intervals.is_empty() {
            return Ok(()); // No files to process
        }

        // Prepare all operations for execution
        let operations_to_execute = self.prepare_delete_operations(
            path_prefix.as_ref(),
            identifier,
            &intervals,
            start,
            end,
        )?;

        if operations_to_execute.is_empty() {
            return Ok(()); // No operations to execute
        }

        // Execute all operations
        let mut files_to_remove = AHashSet::<String>::new();

        for operation in operations_to_execute {
            // Reset the session before each operation
            self.clear_session_tables();

            match operation.kind {
                DeleteOperationKind::SplitBefore | DeleteOperationKind::SplitAfter => {
                    // Query the custom data preserved by the split and write it
                    let instrument_ids = identifier.map(|id| vec![id.to_string()]);
                    let preserved_data = self.query_custom_data_dynamic(
                        type_name,
                        instrument_ids.as_deref(),
                        Some(UnixNanos::from(operation.query_start)),
                        Some(UnixNanos::from(operation.query_end)),
                        None,
                        Some(operation.files.clone()),
                        false,
                    )?;

                    if !preserved_data.is_empty() {
                        let custom_items: Vec<CustomData> = preserved_data
                            .into_iter()
                            .filter_map(|data| match data {
                                Data::Custom(c) => Some(c),
                                _ => None,
                            })
                            .collect();

                        let start_ts = UnixNanos::from(operation.file_start_ns);
                        let end_ts = UnixNanos::from(operation.file_end_ns);

                        for items in group_custom_data_by_type(custom_items.iter()) {
                            self.write_custom_data_refs_batch(
                                &items,
                                Some(start_ts),
                                Some(end_ts),
                                Some(true),
                            )?;
                        }
                    }
                }
                DeleteOperationKind::Remove => {}
            }

            // Mark files for removal (applies to all operation types)
            for file in operation.files {
                files_to_remove.insert(file);
            }
        }

        // Remove all files that were processed
        for file in files_to_remove {
            if let Err(e) = self.delete_file(&file) {
                log::warn!("Failed to delete file {file}: {e}");
            }
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
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
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
    /// catalog.delete_data_range("quotes", Some("BTCUSD"), None, None)?;
    ///
    /// // Delete trade data within a specific time range
    /// catalog.delete_data_range(
    ///     "trades",
    ///     None,
    ///     Some(UnixNanos::from(1609459200000000000)),
    ///     Some(UnixNanos::from(1609545600000000000)),
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn delete_data_range(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()> {
        // Use match statement to call the generic delete_data_range for various types
        match type_name {
            "quotes" => self.delete_data_range_generic::<QuoteTick>(identifier, start, end),
            "trades" => self.delete_data_range_generic::<TradeTick>(identifier, start, end),
            "bars" => self.delete_data_range_generic::<Bar>(identifier, start, end),
            "order_book_deltas" => {
                self.delete_data_range_generic::<OrderBookDelta>(identifier, start, end)
            }
            "order_book_depths" => {
                self.delete_data_range_generic::<OrderBookDepth>(identifier, start, end)
            }
            "mark_prices" => {
                self.delete_data_range_generic::<MarkPriceUpdate>(identifier, start, end)
            }
            "index_prices" => {
                self.delete_data_range_generic::<IndexPriceUpdate>(identifier, start, end)
            }
            "instrument_closes" => {
                self.delete_data_range_generic::<InstrumentClose>(identifier, start, end)
            }
            "funding_rates" => {
                self.delete_data_range_generic::<FundingRateUpdate>(identifier, start, end)
            }
            "option_greeks" => {
                self.delete_data_range_generic::<OptionGreeks>(identifier, start, end)
            }
            "instrument_status" => {
                self.delete_data_range_generic::<InstrumentStatus>(identifier, start, end)
            }
            _ => {
                if type_name.starts_with("custom/") {
                    let custom_type_name = type_name.strip_prefix("custom/").unwrap();
                    self.delete_custom_data_range(custom_type_name, identifier, start, end)
                } else {
                    anyhow::bail!("Unsupported data type: {type_name}");
                }
            }
        }
    }

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
            if let Ok((Some(data_type), identifier)) =
                self.extract_data_cls_and_identifier_from_path(&directory)
            {
                // Call the existing delete_data_range method
                if let Err(e) =
                    self.delete_data_range(&data_type, identifier.as_deref(), start, end)
                {
                    log::warn!("Failed to delete data in directory {directory}: {e}");
                    // Continue with other directories instead of failing completely
                }
            }
        }

        Ok(())
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
    pub fn delete_data_range_generic<T>(
        &mut self,
        identifier: Option<&str>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<()>
    where
        T: DecodeTypedFromRecordBatch
            + CatalogDataType
            + EncodeToRecordBatch
            + HasTsInit
            + TryFrom<Data>
            + Clone,
    {
        // Get intervals for cleaner implementation
        let data_type = T::catalog_data_type();
        let path_prefix = parquet_data_path_prefix(&data_type);
        let intervals = self.get_intervals(path_prefix.as_ref(), identifier)?;

        if intervals.is_empty() {
            return Ok(()); // No files to process
        }

        // Prepare all operations for execution
        let operations_to_execute = self.prepare_delete_operations(
            path_prefix.as_ref(),
            identifier,
            &intervals,
            start,
            end,
        )?;

        if operations_to_execute.is_empty() {
            return Ok(()); // No operations to execute
        }

        // Execute all operations
        let mut files_to_remove = AHashSet::<String>::new();

        for operation in operations_to_execute {
            // Reset the session before each operation to ensure fresh data is loaded
            // This clears any cached table registrations that might interfere with file operations
            self.clear_session_tables();

            match operation.kind {
                DeleteOperationKind::SplitBefore | DeleteOperationKind::SplitAfter => {
                    // Query the data preserved by the split and write it
                    // Use optimize_file_loading=false for precise file control during split operations
                    let instrument_ids = identifier.map(|id| vec![id.to_string()]);
                    let preserved_data = self.query_typed_data::<T>(
                        instrument_ids,
                        Some(UnixNanos::from(operation.query_start)),
                        Some(UnixNanos::from(operation.query_end)),
                        None,
                        Some(operation.files.clone()),
                        false, // optimize_file_loading=false for precise file control
                    )?;

                    if !preserved_data.is_empty() {
                        let start_ts = UnixNanos::from(operation.file_start_ns);
                        let end_ts = UnixNanos::from(operation.file_end_ns);
                        self.write_to_parquet(
                            &preserved_data,
                            Some(start_ts),
                            Some(end_ts),
                            Some(true),
                        )?;
                    }
                }
                DeleteOperationKind::Remove => {}
            }

            // Mark files for removal (applies to all operation types)
            for file in operation.files {
                files_to_remove.insert(file);
            }
        }

        // Remove all files that were processed
        for file in files_to_remove {
            if let Err(e) = self.delete_file(&file) {
                log::warn!("Failed to delete file {file}: {e}");
            }
        }

        Ok(())
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
    pub fn prepare_delete_operations(
        &self,
        type_name: &str,
        identifier: Option<&str>,
        intervals: &[(u64, u64)],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<DeleteOperation>> {
        // Convert start/end to nanoseconds
        let delete_start_ns = start.map(|s| s.as_u64());
        let delete_end_ns = end.map(|e| e.as_u64());

        let mut operations = Vec::new();

        // Get directory for file path construction
        let directory = self.make_path(type_name, identifier)?;

        // Process each interval (which represents an actual file)
        for &(file_start_ns, file_end_ns) in intervals {
            // Check if file intersects with deletion range
            let intersects = delete_start_ns.is_none_or(|start| start <= file_end_ns)
                && delete_end_ns.is_none_or(|end| file_start_ns <= end);

            if !intersects {
                continue; // File doesn't intersect with deletion range
            }

            // Construct file path from interval timestamps
            let filename = timestamps_to_filename(
                UnixNanos::from(file_start_ns),
                UnixNanos::from(file_end_ns),
            );
            let file_path = make_object_store_path(&directory, [&filename]);

            // Determine what type of operation is needed
            let file_completely_within_range = delete_start_ns
                .is_none_or(|start| start <= file_start_ns)
                && delete_end_ns.is_none_or(|end| file_end_ns <= end);

            if file_completely_within_range {
                // File is completely within deletion range - just mark for removal
                operations.push(DeleteOperation {
                    kind: DeleteOperationKind::Remove,
                    files: vec![file_path],
                    query_start: 0,
                    query_end: 0,
                    file_start_ns: 0,
                    file_end_ns: 0,
                });
            } else {
                // File partially overlaps - need to split
                if let Some(delete_start) = delete_start_ns
                    && file_start_ns < delete_start
                {
                    // Keep data before deletion range
                    operations.push(DeleteOperation {
                        kind: DeleteOperationKind::SplitBefore,
                        files: vec![file_path.clone()],
                        query_start: file_start_ns,
                        query_end: delete_start.saturating_sub(1), // Exclusive end
                        file_start_ns,
                        file_end_ns: delete_start.saturating_sub(1),
                    });
                }

                if let Some(delete_end) = delete_end_ns
                    && delete_end < file_end_ns
                {
                    // Keep data after deletion range
                    operations.push(DeleteOperation {
                        kind: DeleteOperationKind::SplitAfter,
                        files: vec![file_path.clone()],
                        query_start: delete_end.saturating_add(1), // Exclusive start
                        query_end: file_end_ns,
                        file_start_ns: delete_end.saturating_add(1),
                        file_end_ns,
                    });
                }
            }
        }

        Ok(operations)
    }
}
