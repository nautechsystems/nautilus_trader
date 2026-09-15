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

use std::sync::Arc;

use datafusion::arrow::{datatypes::Schema, record_batch::RecordBatch};
use futures::StreamExt;
use nautilus_core::{UnixNanos, string::conversions::to_snake_case};
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
        paths::{make_object_store_path, urisafe_instrument_id},
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
        );
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
    /// This method is currently not fully implemented. Feather file reading
    /// requires complex deserialization logic that needs to be added.
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
    /// - `data_cls`: The data class name (e.g., "quotes", "trades", "bars").
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
    /// - The instance ID doesn't exist.
    /// - Feather file listing fails.
    /// - Feather file reading fails.
    /// - Writing to parquet fails.
    ///
    /// # Note
    ///
    /// This method converts directly between Arrow IPC stream batches and Parquet batches without
    /// materializing Nautilus data objects. It requires:
    /// - Listing feather files in the specified subdirectory
    /// - Reading feather files (Arrow IPC stream reading)
    /// - Applying table-only stream conversion transforms
    /// - Writing Arrow batches to the catalog
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
    /// // Convert backtest stream data to parquet
    /// catalog.convert_stream_to_data("instance-123", "quotes", Some("backtest"), None, false)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn convert_stream_to_data(
        &mut self,
        instance_id: &str,
        data_cls: &str,
        subdirectory: Option<&str>,
        identifiers: Option<&[String]>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<()> {
        let subdirectory = subdirectory.unwrap_or("backtest");

        // Convert data class name to filename (e.g., "quotes" -> "quotes")
        // The data_cls should already be in the correct format (snake_case)
        let stream_data_name = to_snake_case(data_cls);
        let catalog_data_name = Self::canonical_stream_data_name(&stream_data_name);

        // List all feather files for this data class
        let feather_files =
            self.list_feather_files(subdirectory, instance_id, &stream_data_name, identifiers)?;

        if feather_files.is_empty() {
            return Ok(());
        }

        if !Self::is_supported_stream_data_type(catalog_data_name) {
            anyhow::bail!("Unknown data class: {data_cls}");
        }

        // Process each feather file independently so that each file's identifier
        // (instrument_id or bar_type from schema metadata) is preserved when writing
        // to parquet. This matches the Python _convert_feather_table_to_parquet approach.
        for file_path in feather_files {
            let batches = self.read_feather_file(&file_path)?;
            self.convert_feather_batches_to_parquet(
                subdirectory,
                instance_id,
                catalog_data_name,
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
        let Some(batch) = Self::apply_stream_conversion_transforms(
            batches,
            use_ts_event_for_ts_init,
        )
        .map_err(|e| {
            anyhow::anyhow!("Failed to apply stream conversion transforms for {feather_path}: {e}")
        })?
        else {
            return Ok(());
        };

        let (start_ts, end_ts) = Self::ts_init_range(&batch).map_err(|e| {
            anyhow::anyhow!("Failed to determine ts_init range for {feather_path}: {e}")
        })?;
        let identifier =
            Self::identifier_from_batch_or_path(&batch, feather_path, subdirectory, instance_id);
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
        let batches = vec![batch];
        self.write_parquet_file_checked(
            &directory,
            UnixNanos::from(start_ts),
            UnixNanos::from(end_ts),
            &batches,
            false,
            "File",
            None,
            replay_identity,
        )?;

        Ok(())
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
                "quotes"
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
