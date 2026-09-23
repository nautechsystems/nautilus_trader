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

//! Feather-file session reading and stream-to-parquet conversion.
//!
//! Methods for reading per-run feather files written by the live/backtest writers and
//! converting them to consolidated parquet for catalog ingest.

#![expect(
    clippy::unused_self,
    reason = "session registration keeps backend-specific ordering logic together"
)]

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use datafusion::arrow::{
    array::{Array, FixedSizeListArray, LargeListArray, ListArray, StructArray, UInt64Array},
    compute::{SortColumn, SortOptions, concat_batches, lexsort_to_indices, take_record_batch},
    datatypes::{DataType, Schema},
    record_batch::RecordBatch,
};
use futures::StreamExt;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, Data, FundingRateUpdate, HasTsInit, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
    OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
    to_variant,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, DecodeTypedFromRecordBatch, U64ColumnRef,
};
use object_store::path::Path as ObjectPath;

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        intervals::are_intervals_disjoint,
        paths::{catalog_filename, make_object_store_path, urisafe_instrument_id},
    },
    catalog::types::{
        CatalogDataType, instrument_path_prefix, parquet_data_path_prefix, record_path_prefix,
    },
    common::{
        conversion::FeatherConversionSummary,
        custom::decode_custom_batches_to_data,
        datafusion::identifiers_from_record_batches,
        paths::{identifier_from_session_feather_path, type_name_from_session_feather_path},
    },
    writer::{
        materializer::{
            StreamConversionOptions, apply_stream_conversion_transform,
            coalesce_stream_conversion_batches, read_feather_record_batches,
            restore_staged_record_batches,
        },
        run::FeatherSessionSource,
    },
};

impl ParquetDataCatalog {
    pub(crate) fn promote_feather_file(
        &self,
        source: &FeatherSessionSource,
        feather_path: &str,
        batches: Vec<RecordBatch>,
        use_ts_event_for_ts_init: bool,
        replay_identity: &str,
    ) -> anyhow::Result<Option<FeatherConversionSummary>> {
        if batches.is_empty() {
            return Ok(None);
        }

        let batches = Self::restore_staged_batches(batches)?;
        let type_name =
            type_name_from_session_feather_path(feather_path, &source.kind, &source.instance_id)?;
        let catalog_data_name = Self::canonical_stream_data_name(&type_name);
        anyhow::ensure!(
            Self::is_supported_stream_data_type(catalog_data_name),
            "Unknown data class: {type_name}"
        );

        let identifier = Self::identifier_from_batch_or_path(
            &batches[0],
            feather_path,
            &source.kind,
            &source.instance_id,
        )
        .filter(|identifier| {
            batches.iter().all(|batch| {
                Self::identifier_from_batch_or_path(
                    batch,
                    feather_path,
                    &source.kind,
                    &source.instance_id,
                )
                .as_ref()
                    == Some(identifier)
            })
        });

        self.convert_feather_batches_to_parquet(
            &source.kind,
            &source.instance_id,
            catalog_data_name,
            feather_path,
            &batches,
            use_ts_event_for_ts_init,
            Some(replay_identity),
        )?;
        Ok(Some(FeatherConversionSummary {
            type_name,
            identifier,
            feather_path: feather_path.to_string(),
            native_version: None,
            unmatched_identifiers: None,
        }))
    }

    /// Reads data from a live run instance.
    ///
    /// This method reads all data associated with a specific live run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the live run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the live run, sorted by timestamp,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instance ID doesn't exist.
    /// - Feather file reading fails.
    /// - Data deserialization fails.
    ///
    /// # Note
    ///
    /// This method reads through the run reader: it lists the run's data-type directories, reads
    /// every Feather file through the Arrow IPC stream reader with staged batch restoration, decodes
    /// quotes, trades, order book deltas and depths, bars, index and mark prices, option Greeks,
    /// funding rates, instrument status and closes, and custom data files into `Data` values, skips
    /// unknown data types, and sorts the result by `ts_init`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
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
    /// // Read data from a live run
    /// let data = catalog.read_live_run("instance-123")?;
    /// for item in data {
    ///     println!("Data: {:?}", item);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn read_live_run(&self, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        self.read_run_data("live", instance_id)
    }

    /// Reads data from a backtest run instance.
    ///
    /// This method reads all data associated with a specific backtest run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the backtest run, sorted by timestamp,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instance ID doesn't exist.
    /// - Feather file reading fails.
    /// - Data deserialization fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
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
    /// // Read data from a backtest run
    /// let data = catalog.read_backtest("instance-123")?;
    /// for item in data {
    ///     println!("Data: {:?}", item);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn read_backtest(&self, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        self.read_run_data("backtest", instance_id)
    }

    /// Helper function to read data from a run instance (backtest or live).
    ///
    /// This function reads all data associated with a specific run instance
    /// from feather files stored in the catalog.
    ///
    /// # Parameters
    ///
    /// - `subdirectory`: The subdirectory name ("backtest" or "live").
    /// - `instance_id`: The ID of the run instance to read.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Data` objects from the run, sorted by timestamp,
    /// or an error if the operation fails.
    fn read_run_data(&self, subdirectory: &str, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        // List all data type directories in the instance directory
        let data_types = self.list_directory_stems(&format!("{subdirectory}/{instance_id}"))?;

        if data_types.is_empty() {
            // No data types found - return empty vector
            return Ok(Vec::new());
        }

        let mut all_data: Vec<Data> = Vec::new();

        // Process each persisted data type.
        for data_cls in data_types {
            // List all feather files for this data type
            let feather_files = self.list_feather_files(
                subdirectory,
                instance_id,
                &data_cls,
                None, // No identifier filtering - read all
            )?;

            if feather_files.is_empty() {
                continue; // Skip if no files found
            }

            // Process each feather file
            for file_path in feather_files {
                // Read the feather file (may contain multiple batches)
                let batches = self.read_feather_file(&file_path)?;

                if batches.is_empty() {
                    continue; // Skip empty or invalid files
                }

                let decode_data_cls = Self::canonical_stream_data_name(&data_cls);

                // Convert RecordBatches to Data objects based on data_cls
                let file_data: Vec<Data> = match decode_data_cls {
                    "quotes" => {
                        let quotes: Vec<QuoteTick> =
                            self.convert_record_batches_to_data(batches, false)?;
                        quotes.into_iter().map(Data::from).collect()
                    }
                    "trades" => {
                        let trades: Vec<TradeTick> =
                            self.convert_record_batches_to_data(batches, false)?;
                        trades.into_iter().map(Data::from).collect()
                    }
                    "order_book_deltas" => {
                        let deltas: Vec<OrderBookDelta> =
                            self.convert_record_batches_to_data(batches, false)?;
                        deltas.into_iter().map(Data::from).collect()
                    }
                    "order_book_depths" => {
                        let depths: Vec<OrderBookDepth> =
                            self.convert_record_batches_to_data(batches, false)?;
                        depths.into_iter().map(Data::from).collect()
                    }
                    "bars" => {
                        let bars: Vec<Bar> = self.convert_record_batches_to_data(batches, false)?;
                        bars.into_iter().map(Data::from).collect()
                    }
                    "index_prices" => {
                        let prices: Vec<IndexPriceUpdate> =
                            self.convert_record_batches_to_data(batches, false)?;
                        prices.into_iter().map(Data::from).collect()
                    }
                    "mark_prices" => {
                        let prices: Vec<MarkPriceUpdate> =
                            self.convert_record_batches_to_data(batches, false)?;
                        prices.into_iter().map(Data::from).collect()
                    }
                    "option_greeks" => {
                        let greeks: Vec<OptionGreeks> =
                            self.convert_record_batches_to_data(batches, false)?;
                        greeks.into_iter().map(Data::from).collect()
                    }
                    "funding_rates" => {
                        let funding_rates: Vec<FundingRateUpdate> =
                            self.convert_record_batches_to_data(batches, false)?;
                        funding_rates.into_iter().map(Data::from).collect()
                    }
                    "instrument_status" => {
                        let statuses: Vec<InstrumentStatus> =
                            self.convert_record_batches_to_data(batches, false)?;
                        statuses.into_iter().map(Data::from).collect()
                    }
                    "instrument_closes" => {
                        let closes: Vec<InstrumentClose> =
                            self.convert_record_batches_to_data(batches, false)?;
                        closes.into_iter().map(Data::from).collect()
                    }
                    _ => {
                        if decode_data_cls.starts_with("custom/") {
                            decode_custom_batches_to_data(batches, false)?
                        } else {
                            // Unknown data type - skip it
                            continue;
                        }
                    }
                };

                all_data.extend(file_data);
            }
        }

        // Sort all data by timestamp (ts_init)
        all_data.sort_by_key(HasTsInit::ts_init);

        Ok(all_data)
    }

    /// Lists feather files for a specific data class in a subdirectory.
    ///
    /// This function finds all `.feather` files in the specified subdirectory
    /// (backtest or live) for the given instance ID and data class.
    fn list_feather_files(
        &self,
        subdirectory: &str,
        instance_id: &str,
        data_name: &str,
        identifiers: Option<&[String]>,
    ) -> anyhow::Result<Vec<String>> {
        let base_dir = make_object_store_path(&self.base_path, [subdirectory, instance_id]);

        let mut files = Vec::new();

        let list_result = self.execute_async(|| async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut feather_files = Vec::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();

                if !path_str.ends_with(".feather") {
                    continue;
                }

                let Ok(path_data_name) =
                    type_name_from_session_feather_path(&path_str, subdirectory, instance_id)
                else {
                    continue;
                };

                if path_data_name != data_name {
                    continue;
                }

                let path_identifier =
                    identifier_from_session_feather_path(&path_str, subdirectory, instance_id);

                if let (Some(identifiers), Some(path_identifier)) =
                    (identifiers, path_identifier.as_deref())
                    && !Self::stream_identifier_matches(path_identifier, identifiers)
                {
                    continue;
                }

                feather_files.push(path_str);
            }

            Ok::<Vec<String>, anyhow::Error>(feather_files)
        })?;

        files.extend(list_result);
        files.sort();
        Ok(files)
    }

    fn stream_identifier_matches(candidate: &str, identifiers: &[String]) -> bool {
        identifiers.iter().any(|id| {
            let safe_id = urisafe_instrument_id(id);
            candidate.contains(id) || candidate.contains(&safe_id)
        })
    }

    /// Reads a feather file and returns all `RecordBatches`.
    fn read_feather_file(&self, file_path: &str) -> anyhow::Result<Vec<RecordBatch>> {
        let path = ObjectPath::from(file_path);

        let batches = self.execute_async(|| async {
            read_feather_record_batches(self.object_store.clone(), &path).await
        })?;

        Self::restore_staged_batches(batches)
    }

    fn restore_staged_batches(batches: Vec<RecordBatch>) -> anyhow::Result<Vec<RecordBatch>> {
        let mut restored = Vec::new();
        for batch in batches {
            restored.extend(restore_staged_record_batches(batch)?);
        }

        Ok(restored)
    }

    /// Converts `RecordBatches` to Data objects, optionally replacing `ts_init` with `ts_event`.
    fn convert_record_batches_to_data<T>(
        &self,
        batches: Vec<RecordBatch>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeDataFromRecordBatch + TryFrom<Data>,
    {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_data = Vec::new();

        for batch in batches {
            let batch = apply_stream_conversion_transform(
                &batch,
                StreamConversionOptions {
                    use_ts_event_for_ts_init,
                    convert_bar_type_to_external: false,
                },
            )?;

            let metadata = batch.schema().metadata().clone();

            let data_vec = T::decode_data_batch(&metadata, batch)
                .map_err(|e| anyhow::anyhow!("Failed to decode batch: {e}"))?;

            all_data.extend(data_vec);
        }

        Ok(to_variant::<T>(all_data))
    }

    /// Converts `RecordBatches` directly to strongly typed values.
    pub(crate) fn convert_record_batches_to_typed<T>(
        &self,
        batches: Vec<RecordBatch>,
    ) -> anyhow::Result<Vec<T>>
    where
        T: DecodeTypedFromRecordBatch,
    {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_data = Vec::new();

        for batch in batches {
            let metadata = batch.schema().metadata().clone();
            let decoded = T::decode_typed_batch(&metadata, batch)
                .map_err(|e| anyhow::anyhow!("Failed to decode batch: {e}"))?;
            all_data.extend(decoded);
        }

        Ok(all_data)
    }

    /// Converts stream data from feather files to parquet files.
    ///
    /// This method reads data from feather files generated during a backtest or live run
    /// and writes it to the catalog in parquet format. It's useful for converting temporary
    /// stream data into a more permanent and queryable format.
    ///
    /// # Parameters
    ///
    /// - `instance_id`: The ID of the backtest or live run instance.
    /// - `data_cls`: The data class name (e.g., "quotes", "trades", "bars"), or
    ///   `custom/{TypeName}` with the registered type name verbatim for custom data.
    /// - `subdirectory`: The subdirectory containing the feather files. Either "backtest" or "live" (default: "backtest").
    /// - `identifiers`: Optional list of identifiers to filter by (instrument IDs or bar types).
    /// - `use_ts_event_for_ts_init`: If true, replaces the `ts_init` column with `ts_event` column values before deserializing.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `data_type` is an instrument class selector, which has no staged stream name.
    /// - `data_type` is a family streams do not support.
    /// - Feather file listing fails.
    /// - Feather file reading fails.
    /// - Writing to parquet fails.
    ///
    /// # Note
    ///
    /// This method converts directly between Arrow IPC stream batches and Parquet batches without
    /// materializing Nautilus data objects. An instance with no staged files for the family
    /// converts nothing and returns success. It requires:
    /// - Listing feather files in the specified subdirectory
    /// - Reading feather files (Arrow IPC stream reading)
    /// - Applying table-only stream conversion transforms
    /// - Writing Arrow batches to the catalog
    ///
    /// # Examples
    ///
    /// ```rust,no_run
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
    /// // Convert backtest stream data to parquet
    /// catalog.convert_stream_to_data(
    ///     "instance-123",
    ///     &NautilusDataType::QuoteTick.into(),
    ///     Some("backtest"),
    ///     None,
    ///     false,
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn convert_stream_to_data(
        &mut self,
        instance_id: &str,
        data_type: &CatalogDataType,
        subdirectory: Option<&str>,
        identifiers: Option<&[String]>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<()> {
        let subdirectory = subdirectory.unwrap_or("backtest");

        // Streams stage instruments under the single aggregate name,
        // with the class carried per batch, so a class selector names no staged directory.
        let stream_data_name: Cow<'static, str> = match data_type {
            CatalogDataType::Data(data_type) => parquet_data_path_prefix(data_type),
            CatalogDataType::Record(record_type) => record_path_prefix(record_type),
            CatalogDataType::Instrument(class) => {
                anyhow::bail!(
                    "Stream conversion stages instruments under the aggregate family, not {class}; \
                     pass the Instrument data type"
                );
            }
        };

        if !Self::is_supported_stream_data_type(&stream_data_name) {
            anyhow::bail!("Stream conversion does not support {data_type}");
        }

        // List all feather files for this data class
        let feather_files =
            self.list_feather_files(subdirectory, instance_id, &stream_data_name, identifiers)?;

        if feather_files.is_empty() {
            return Ok(());
        }

        // Process each feather file independently so that each file's identifier
        // (instrument_id or bar_type from schema metadata) is preserved when writing
        // to parquet. Each file is planned before it is written.
        for file_path in feather_files {
            let batches = self.read_feather_file(&file_path)?;
            self.convert_feather_batches_to_parquet(
                subdirectory,
                instance_id,
                &stream_data_name,
                &file_path,
                &batches,
                use_ts_event_for_ts_init,
                None,
            )?;
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments describe one Feather source and its catalog destination"
    )]
    fn convert_feather_batches_to_parquet(
        &self,
        subdirectory: &str,
        instance_id: &str,
        catalog_data_name: &str,
        feather_path: &str,
        batches: &[RecordBatch],
        use_ts_event_for_ts_init: bool,
        replay_identity: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut groups: IndexMap<Arc<Schema>, Vec<RecordBatch>> = IndexMap::new();

        for batch in batches {
            for restored in restore_staged_record_batches(batch.clone())? {
                groups.entry(restored.schema()).or_default().push(restored);
            }
        }

        let mut planned = Vec::new();

        for (index, group) in groups.into_values().enumerate() {
            if let Some(plan) = self.plan_catalog_write(
                subdirectory,
                instance_id,
                catalog_data_name,
                feather_path,
                &group,
                use_ts_event_for_ts_init,
                replay_identity,
                index,
            )? {
                planned.push(plan);
            }
        }

        let mut by_directory: IndexMap<String, Vec<PlannedCatalogWrite>> = IndexMap::new();

        for plan in planned {
            by_directory
                .entry(plan.directory.clone())
                .or_default()
                .push(plan);
        }

        let mut ready = Vec::new();

        for (directory, plans) in by_directory {
            ready.extend(self.ready_directory_plans(&directory, plans)?);
        }

        for plan in ready {
            self.write_parquet_file_checked(
                &plan.directory,
                UnixNanos::from(plan.start_ts),
                UnixNanos::from(plan.end_ts),
                std::slice::from_ref(&plan.batch),
                false,
                "File",
                None,
                Some(&plan.group_identity),
            )?;
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the arguments describe one restored schema group and its catalog destination"
    )]
    fn plan_catalog_write(
        &self,
        subdirectory: &str,
        instance_id: &str,
        catalog_data_name: &str,
        feather_path: &str,
        group: &[RecordBatch],
        use_ts_event_for_ts_init: bool,
        replay_identity: Option<&str>,
        index: usize,
    ) -> anyhow::Result<Option<PlannedCatalogWrite>> {
        let Some(batch) = Self::apply_stream_conversion_transforms(group, use_ts_event_for_ts_init)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to apply stream conversion transforms for {feather_path}: {e}"
                )
            })?
        else {
            return Ok(None);
        };

        let (start_ts, end_ts) = Self::ts_init_range(&batch).map_err(|e| {
            anyhow::anyhow!("Failed to determine ts_init range for {feather_path}: {e}")
        })?;

        let identifier =
            Self::identifier_from_batch_or_path(&batch, feather_path, subdirectory, instance_id);

        let instrument_prefix = if catalog_data_name == "instruments" {
            let class = batch
                .schema()
                .metadata()
                .get("class")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Staged instrument has no class metadata"))?;
            Some(instrument_path_prefix(&class.parse()?))
        } else {
            None
        };

        let catalog_data_name = instrument_prefix.unwrap_or(catalog_data_name);

        let directory = if let Some(type_name) = catalog_data_name.strip_prefix("custom/") {
            self.make_path_custom_data(type_name, identifier.as_deref())?
        } else {
            self.make_path(catalog_data_name, identifier.as_deref())?
        };

        let batch = Self::with_catalog_identifier_metadata(
            batch,
            catalog_data_name,
            identifier.as_deref(),
        )?;

        Ok(Some(PlannedCatalogWrite {
            directory,
            start_ts,
            end_ts,
            batch,
            group_identity: format!("{}/{index}", replay_identity.unwrap_or(feather_path)),
        }))
    }

    fn ready_directory_plans(
        &self,
        directory: &str,
        plans: Vec<PlannedCatalogWrite>,
    ) -> anyhow::Result<Vec<PlannedCatalogWrite>> {
        let plans = coalesce_overlapping_plans(plans)?;
        let mut remaining = Vec::new();

        for plan in plans {
            let filename = catalog_filename(
                UnixNanos::from(plan.start_ts),
                UnixNanos::from(plan.end_ts),
                Some(&plan.group_identity),
            );
            let path = format!("{directory}/{filename}");
            if !self.file_exists(&path)? {
                remaining.push(plan);
            }
        }

        if remaining.is_empty() {
            return Ok(remaining);
        }

        let existing = self.get_directory_intervals(directory)?;
        let plans = remaining;
        let mut intervals = existing.clone();
        intervals.extend(plans.iter().map(|plan| (plan.start_ts, plan.end_ts)));
        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!(
                "Writing promoted groups for {directory} would create non-disjoint intervals. \
                 Existing intervals: {existing:?}"
            );
        }

        Ok(plans)
    }

    fn with_catalog_identifier_metadata(
        batch: RecordBatch,
        catalog_data_name: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<RecordBatch> {
        let Some(identifier) = identifier else {
            return Ok(batch);
        };

        let metadata_key = if catalog_data_name == "bars" {
            "bar_type"
        } else {
            "instrument_id"
        };

        if batch.schema().metadata().contains_key(metadata_key) {
            return Ok(batch);
        }

        let mut metadata = batch.schema().metadata().clone();
        metadata.insert(metadata_key.to_string(), identifier.to_string());

        let schema = Arc::new(Schema::new_with_metadata(
            batch.schema().fields().clone(),
            metadata,
        ));
        Ok(RecordBatch::try_new(schema, batch.columns().to_vec())?)
    }

    fn apply_stream_conversion_transforms(
        batches: &[RecordBatch],
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<Option<RecordBatch>> {
        coalesce_stream_conversion_batches(
            batches,
            StreamConversionOptions {
                use_ts_event_for_ts_init,
                convert_bar_type_to_external: true,
            },
        )
    }

    fn ts_init_range(batch: &RecordBatch) -> anyhow::Result<(u64, u64)> {
        let ts_init = Self::ts_init_array(batch)?;
        if ts_init.is_empty() {
            anyhow::bail!("Cannot convert empty stream batch to parquet");
        }

        if (0..ts_init.len()).any(|row| ts_init.is_null(row)) {
            anyhow::bail!("ts_init column contains null values");
        }

        let start = ts_init
            .value(0)
            .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))?;
        let end = ts_init
            .value(ts_init.len() - 1)
            .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))?;
        Ok((start, end))
    }

    fn ts_init_array(batch: &RecordBatch) -> anyhow::Result<U64ColumnRef<'_>> {
        let ts_init_idx = batch
            .schema()
            .index_of("ts_init")
            .map_err(|_| anyhow::anyhow!("ts_init column not found"))?;
        U64ColumnRef::try_from_array(batch.column(ts_init_idx).as_ref())
            .ok_or_else(|| anyhow::anyhow!("ts_init column has an unsupported type"))
    }

    fn identifier_from_batch_or_path(
        batch: &RecordBatch,
        feather_path: &str,
        subdirectory: &str,
        instance_id: &str,
    ) -> Option<String> {
        let metadata = batch.schema().metadata().clone();
        if let Some(bar_type) = metadata.get("bar_type") {
            return Some(bar_type.clone());
        }

        if let Some(instrument_id) = metadata.get("instrument_id") {
            return Some(instrument_id.clone());
        }

        if let Ok(identifiers) = identifiers_from_record_batches(std::slice::from_ref(batch))
            && identifiers.len() == 1
        {
            return identifiers.into_iter().next();
        }

        identifier_from_session_feather_path(feather_path, subdirectory, instance_id)
    }

    fn canonical_stream_data_name(data_name: &str) -> &str {
        match data_name {
            "quote_tick" => "quotes",
            "trade_tick" => "trades",
            "bar" => "bars",
            "mark_price_update" => "mark_prices",
            "index_price_update" => "index_prices",
            "funding_rate_update" => "funding_rates",
            "instrument_close" => "instrument_closes",
            "order_book_delta" => "order_book_deltas",
            other => other,
        }
    }

    fn is_supported_stream_data_type(data_name: &str) -> bool {
        data_name.starts_with("custom/")
            || matches!(
                data_name,
                "instruments"
                    | "quotes"
                    | "trades"
                    | "order_book_deltas"
                    | "order_book_depths"
                    | "bars"
                    | "index_prices"
                    | "mark_prices"
                    | "option_greeks"
                    | "instrument_status"
                    | "instrument_closes"
                    | "funding_rates"
                    | "account_state"
                    | "order_initialized"
                    | "order_denied"
                    | "order_emulated"
                    | "order_submitted"
                    | "order_accepted"
                    | "order_rejected"
                    | "order_pending_cancel"
                    | "order_canceled"
                    | "order_cancel_rejected"
                    | "order_expired"
                    | "order_triggered"
                    | "order_pending_update"
                    | "order_released"
                    | "order_modify_rejected"
                    | "order_updated"
                    | "order_filled"
                    | "order_fill_voided"
                    | "position_opened"
                    | "position_changed"
                    | "position_closed"
                    | "position_adjusted"
                    | "order_snapshot"
                    | "position_snapshot"
                    | "order_status_report"
                    | "fill_report"
                    | "position_status_report"
                    | "execution_mass_status"
            )
    }
}

struct PlannedCatalogWrite {
    directory: String,
    start_ts: u64,
    end_ts: u64,
    batch: RecordBatch,
    group_identity: String,
}

fn coalesce_overlapping_plans(
    mut plans: Vec<PlannedCatalogWrite>,
) -> anyhow::Result<Vec<PlannedCatalogWrite>> {
    loop {
        let mut changed = false;
        let mut next = Vec::new();

        while let Some(plan) = plans.pop() {
            if let Some(index) = next
                .iter()
                .position(|other| plan_intervals_overlap(other, &plan))
            {
                let other = next.swap_remove(index);
                next.push(unify_plans(other, plan)?);
                changed = true;
            } else {
                next.push(plan);
            }
        }

        plans = next;

        if !changed {
            break;
        }
    }

    Ok(plans)
}

fn plan_intervals_overlap(left: &PlannedCatalogWrite, right: &PlannedCatalogWrite) -> bool {
    left.start_ts <= right.end_ts && right.start_ts <= left.end_ts
}

fn unify_plans(
    left: PlannedCatalogWrite,
    right: PlannedCatalogWrite,
) -> anyhow::Result<PlannedCatalogWrite> {
    let batch = unify_record_batches(left.batch, right.batch)?;
    let (start_ts, end_ts) = min_max_ts_init(&batch)?;

    Ok(PlannedCatalogWrite {
        directory: left.directory,
        start_ts,
        end_ts,
        batch,
        group_identity: left.group_identity,
    })
}

fn unify_record_batches(left: RecordBatch, right: RecordBatch) -> anyhow::Result<RecordBatch> {
    if left.schema() == right.schema() {
        return concat_sorted(&left, &right);
    }

    anyhow::ensure!(
        fields_compatible(left.schema().as_ref(), right.schema().as_ref())
            && metadata_without_precision(left.schema().as_ref())
                == metadata_without_precision(right.schema().as_ref()),
        "overlapping promotion groups have incompatible schemas"
    );

    let target = precision_target_schema(left.schema().as_ref(), right.schema().as_ref())?;
    let left = relabel_precision(left, &target)?;
    let right = relabel_precision(right, &target)?;
    concat_sorted(&left, &right)
}

fn precision_target_schema(left: &Schema, right: &Schema) -> anyhow::Result<Schema> {
    if precision_values(left) == precision_values(right) {
        return Ok(left.clone());
    }

    if is_precision_sentinel(left) && !is_precision_sentinel(right) {
        return Ok(right.clone());
    }

    if is_precision_sentinel(right) && !is_precision_sentinel(left) {
        return Ok(left.clone());
    }

    anyhow::bail!("overlapping promotion groups have incompatible precision metadata")
}

fn relabel_precision(batch: RecordBatch, target: &Schema) -> anyhow::Result<RecordBatch> {
    if batch.schema().as_ref() == target {
        return Ok(batch);
    }

    let precision_changes = precision_values(batch.schema().as_ref()) != precision_values(target);
    if precision_changes && batch_has_present_decimal(&batch) {
        anyhow::bail!(
            "cannot relabel precision metadata for a promotion group that contains decimal values"
        );
    }

    Ok(RecordBatch::try_new(
        Arc::new(target.clone()),
        batch.columns().to_vec(),
    )?)
}

fn concat_sorted(left: &RecordBatch, right: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let schema = left.schema();
    let batch = concat_batches(&schema, [left, right])
        .map_err(|e| anyhow::anyhow!("Failed to concatenate promotion groups: {e}"))?;
    sort_by_ts_init(&batch)
}

fn sort_by_ts_init(batch: &RecordBatch) -> anyhow::Result<RecordBatch> {
    let ts_init = batch
        .schema()
        .index_of("ts_init")
        .map_err(|_| anyhow::anyhow!("ts_init column not found"))?;
    let original_row_index = Arc::new(UInt64Array::from_iter_values(0..batch.num_rows() as u64));

    let indices = lexsort_to_indices(
        &[
            SortColumn {
                values: batch.column(ts_init).clone(),
                options: Some(SortOptions {
                    descending: false,
                    nulls_first: false,
                }),
            },
            SortColumn {
                values: original_row_index,
                options: Some(SortOptions {
                    descending: false,
                    nulls_first: false,
                }),
            },
        ],
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to sort promotion group: {e}"))?;

    take_record_batch(batch, &indices)
        .map_err(|e| anyhow::anyhow!("Failed to reorder promotion group: {e}"))
}

fn min_max_ts_init(batch: &RecordBatch) -> anyhow::Result<(u64, u64)> {
    let ts_init = U64ColumnRef::try_from_array(
        batch
            .column_by_name("ts_init")
            .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?
            .as_ref(),
    )
    .ok_or_else(|| anyhow::anyhow!("ts_init column has an unsupported type"))?;

    if ts_init.is_empty() {
        anyhow::bail!("Cannot convert empty stream batch to parquet");
    }

    let mut min = u64::MAX;
    let mut max = 0_u64;

    for row in 0..ts_init.len() {
        anyhow::ensure!(!ts_init.is_null(row), "ts_init column contains null values");
        let value = ts_init
            .value(row)
            .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))?;
        min = min.min(value);
        max = max.max(value);
    }

    Ok((min, max))
}

fn fields_compatible(left: &Schema, right: &Schema) -> bool {
    left.fields().len() == right.fields().len()
        && left
            .fields()
            .iter()
            .zip(right.fields())
            .all(|(left, right)| {
                left.name() == right.name()
                    && left.data_type() == right.data_type()
                    && left.is_nullable() == right.is_nullable()
                    && left.metadata() == right.metadata()
            })
}

fn metadata_without_precision(schema: &Schema) -> HashMap<String, String> {
    schema
        .metadata()
        .iter()
        .filter(|(key, _)| *key != "price_precision" && *key != "size_precision")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn precision_values(schema: &Schema) -> (Option<&str>, Option<&str>) {
    (
        schema.metadata().get("price_precision").map(String::as_str),
        schema.metadata().get("size_precision").map(String::as_str),
    )
}

fn is_precision_sentinel(schema: &Schema) -> bool {
    let (price, size) = precision_values(schema);
    (price.is_some() || size.is_some())
        && price.is_none_or(|value| value == "0")
        && size.is_none_or(|value| value == "0")
}

fn batch_has_present_decimal(batch: &RecordBatch) -> bool {
    batch
        .columns()
        .iter()
        .any(|column| array_has_present_decimal(column.as_ref()))
}

fn array_has_present_decimal(array: &dyn Array) -> bool {
    match array.data_type() {
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => {
            !array.is_empty() && array.null_count() < array.len()
        }
        DataType::List(_) => array
            .as_any()
            .downcast_ref::<ListArray>()
            .is_none_or(|list| array_has_present_decimal(list.values().as_ref())),
        DataType::LargeList(_) => array
            .as_any()
            .downcast_ref::<LargeListArray>()
            .is_none_or(|list| array_has_present_decimal(list.values().as_ref())),
        DataType::FixedSizeList(_, _) => array
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .is_none_or(|list| array_has_present_decimal(list.values().as_ref())),
        DataType::Struct(_) => array
            .as_any()
            .downcast_ref::<StructArray>()
            .is_none_or(|values| {
                values
                    .columns()
                    .iter()
                    .any(|column| array_has_present_decimal(column.as_ref()))
            }),
        _ => false,
    }
}

#[cfg(test)]
mod promotion_group_tests {
    use std::{collections::HashMap, sync::Arc};

    use datafusion::arrow::{
        array::{Array, Decimal128Array, UInt64Array},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_model::data::NautilusDataType;
    use rstest::rstest;
    use tempfile::TempDir;

    use crate::backend::parquet::catalog::ParquetDataCatalog;

    fn precision_batch(precision: &str, timestamps: Vec<u64>, price: Option<i128>) -> RecordBatch {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), "ETH/USDT.BINANCE".to_string());
        metadata.insert("price_precision".to_string(), precision.to_string());
        metadata.insert("size_precision".to_string(), "0".to_string());
        let price = Decimal128Array::from(vec![price; timestamps.len()])
            .with_precision_and_scale(38, 16)
            .unwrap();
        RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new("price", price.data_type().clone(), true),
                ],
                metadata,
            )),
            vec![Arc::new(UInt64Array::from(timestamps)), Arc::new(price)],
        )
        .unwrap()
    }

    #[rstest]
    fn overlapping_empty_precision_group_is_promoted() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let batches = vec![
            precision_batch("0", vec![1, 3], None),
            precision_batch("2", vec![2], Some(20_000_000_000_000_000)),
        ];

        catalog
            .convert_feather_batches_to_parquet(
                "backtest",
                "run-1",
                "quotes",
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap();
        catalog
            .convert_feather_batches_to_parquet(
                "backtest",
                "run-1",
                "quotes",
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap();

        assert_eq!(
            catalog
                .get_intervals(
                    &NautilusDataType::QuoteTick.into(),
                    Some("ETH/USDT.BINANCE")
                )
                .unwrap(),
            vec![(1, 3)],
        );
    }

    #[rstest]
    fn overlapping_incompatible_groups_write_nothing() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let batches = vec![
            precision_batch("2", vec![1, 3], Some(1)),
            precision_batch("5", vec![2], Some(2)),
        ];

        let error = catalog
            .convert_feather_batches_to_parquet(
                "backtest",
                "run-1",
                "quotes",
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap_err();
        assert!(error.to_string().contains("incompatible"));
        assert!(
            catalog
                .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod canonical_name_tests {
    use rstest::rstest;

    use super::ParquetDataCatalog;

    #[rstest]
    #[case("quotes", "quotes")]
    #[case("trades", "trades")]
    #[case("bars", "bars")]
    #[case("order_book_delta", "order_book_deltas")]
    #[case("mark_prices", "mark_prices")]
    #[case("index_prices", "index_prices")]
    #[case("funding_rates", "funding_rates")]
    #[case("instrument_closes", "instrument_closes")]
    #[case("quotes", "quotes")]
    fn canonical_stream_data_aliases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            ParquetDataCatalog::canonical_stream_data_name(input),
            expected,
        );
    }
}
