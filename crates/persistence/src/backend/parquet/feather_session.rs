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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use ahash::AHashMap;
use datafusion::arrow::{
    array::{
        Array, ArrayRef, FixedSizeListArray, GenericListArray, LargeListArray, ListArray,
        OffsetSizeTrait, StructArray, UInt64Array,
    },
    compute::{SortColumn, SortOptions, concat_batches, lexsort_to_indices, take_record_batch},
    datatypes::{DataType, Schema},
    record_batch::RecordBatch,
};
use futures::StreamExt;
use indexmap::IndexMap;
use nautilus_common::enums::Environment;
use nautilus_core::UnixNanos;
#[cfg(feature = "defi")]
use nautilus_model::data::NautilusRecordType;
use nautilus_model::data::{
    Bar, Data, FundingRateUpdate, HasTsInit, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
    NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
    close::InstrumentClose, to_variant,
};
use nautilus_serialization::arrow::{
    DecodeDataFromRecordBatch, DecodeTypedFromRecordBatch, KEY_TYPE_NAME, U64ColumnRef,
    record_batch_without_identifier_column,
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
        datafusion::{filter_record_batch_by_identifier, identifiers_from_record_batches},
        paths::{
            catalog_data_type_from_session_feather_path, environment_directory,
            identifier_from_session_feather_path,
        },
    },
    writer::{
        feather::recover_partial_feather_files,
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
        let batches = Self::restore_staged_batches(batches)?;
        if batches.is_empty() {
            return Ok(None);
        }

        let data_type = catalog_data_type_from_session_feather_path(
            feather_path,
            source.environment,
            &source.instance_id,
        )?;
        Self::ensure_stream_data_type(&data_type)?;

        let identifier = Self::identifier_from_batch_or_path(
            &batches[0],
            feather_path,
            source.environment,
            &source.instance_id,
        )
        .filter(|identifier| {
            batches.iter().all(|batch| {
                Self::identifier_from_batch_or_path(
                    batch,
                    feather_path,
                    source.environment,
                    &source.instance_id,
                )
                .as_ref()
                    == Some(identifier)
            })
        });

        self.convert_feather_batches_to_parquet(
            source.environment,
            &source.instance_id,
            &data_type,
            feather_path,
            &batches,
            use_ts_event_for_ts_init,
            Some(replay_identity),
        )?;
        Ok(Some(FeatherConversionSummary {
            data_type,
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
        self.read_run(Environment::Live, instance_id)
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
        self.read_run(Environment::Backtest, instance_id)
    }

    /// Reads the sealed Feather files of a run instance (backtest or live).
    ///
    /// Abandoned `.feather.partial` files are recovered first, and files a writer still holds
    /// open are not read. Families that do not decode to `Data`, such as order events, are
    /// skipped.
    fn read_run(&self, environment: Environment, instance_id: &str) -> anyhow::Result<Vec<Data>> {
        self.recover_partial_run_files(environment, instance_id);
        let feather_files = self.list_feather_files(environment, instance_id, None, None)?;
        let mut all_data: Vec<Data> = Vec::new();

        for file_path in feather_files {
            let data_type =
                catalog_data_type_from_session_feather_path(&file_path, environment, instance_id)?;

            // A file may hold several batches
            let batches = self.read_feather_file(&file_path)?;

            if batches.is_empty() {
                continue;
            }

            if let Some(file_data) = self.decode_run_batches(&data_type, batches)? {
                all_data.extend(file_data);
            }
        }

        all_data.sort_by_key(HasTsInit::ts_init);

        Ok(all_data)
    }

    // Returns `None` for stream families that do not decode to `Data`
    fn decode_run_batches(
        &self,
        data_type: &CatalogDataType,
        batches: Vec<RecordBatch>,
    ) -> anyhow::Result<Option<Vec<Data>>> {
        let data = match data_type {
            CatalogDataType::Data(NautilusDataType::QuoteTick) => self
                .convert_record_batches_to_data::<QuoteTick>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::TradeTick) => self
                .convert_record_batches_to_data::<TradeTick>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::OrderBookDelta) => self
                .convert_record_batches_to_data::<OrderBookDelta>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::OrderBookDepth) => self
                .convert_record_batches_to_data::<OrderBookDepth>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::Bar) => self
                .convert_record_batches_to_data::<Bar>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::IndexPriceUpdate) => self
                .convert_record_batches_to_data::<IndexPriceUpdate>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::MarkPriceUpdate) => self
                .convert_record_batches_to_data::<MarkPriceUpdate>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::OptionGreeks) => self
                .convert_record_batches_to_data::<OptionGreeks>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::FundingRateUpdate) => self
                .convert_record_batches_to_data::<FundingRateUpdate>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::InstrumentStatus) => self
                .convert_record_batches_to_data::<InstrumentStatus>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::InstrumentClose) => self
                .convert_record_batches_to_data::<InstrumentClose>(batches, false)?
                .into_iter()
                .map(Data::from)
                .collect(),
            CatalogDataType::Data(NautilusDataType::Custom { .. }) => {
                decode_custom_batches_to_data(batches, false)?
            }
            _ => return Ok(None),
        };

        Ok(Some(data))
    }

    // Writers stage only to local directories, so other catalogs hold no partial files
    fn recover_partial_run_files(&self, environment: Environment, instance_id: &str) {
        if self.original_uri.starts_with("file://") {
            let directory = PathBuf::from(self.native_base_path_string())
                .join(environment_directory(environment))
                .join(instance_id);
            recover_partial_feather_files(&directory);
        }
    }

    /// Lists the feather files of a run, for one data type or every type when `data_type` is
    /// `None`.
    fn list_feather_files(
        &self,
        environment: Environment,
        instance_id: &str,
        data_type: Option<&CatalogDataType>,
        identifiers: Option<&[String]>,
    ) -> anyhow::Result<Vec<String>> {
        let base_dir = make_object_store_path(
            &self.base_path,
            [environment_directory(environment), instance_id],
        );

        let mut files = self.execute_async(|| async {
            let prefix = ObjectPath::from(format!("{base_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut feather_files = Vec::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();

                if !path_str.ends_with(".feather") {
                    continue;
                }

                let Ok(path_data_type) = catalog_data_type_from_session_feather_path(
                    &path_str,
                    environment,
                    instance_id,
                ) else {
                    continue;
                };

                if data_type.is_some_and(|data_type| path_data_type != *data_type) {
                    continue;
                }

                let path_identifier =
                    identifier_from_session_feather_path(&path_str, environment, instance_id);

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

        files.sort();
        Ok(files)
    }

    // Keeps the rows whose identifier matches; a type's file holds every identifier, so the file
    // listing cannot select them by folder
    fn filter_batches_by_identifiers(
        batches: Vec<RecordBatch>,
        file_path: &str,
        environment: Environment,
        instance_id: &str,
        identifiers: Option<&[String]>,
    ) -> anyhow::Result<Vec<RecordBatch>> {
        let Some(identifiers) = identifiers else {
            return Ok(batches);
        };

        let path_identifier =
            identifier_from_session_feather_path(file_path, environment, instance_id);

        batches
            .iter()
            .filter_map(|batch| {
                filter_record_batch_by_identifier(batch, path_identifier.as_deref(), |identifier| {
                    identifier.is_none_or(|identifier| {
                        Self::stream_identifier_matches(identifier, identifiers)
                    })
                })
                .transpose()
            })
            .collect()
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
    /// - `data_type`: The data or record type to convert, with the registered type name verbatim
    ///   for custom data.
    /// - `environment`: The environment of the run, which names the folder holding its feather files.
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
    /// - Recovering abandoned `.feather.partial` files of a local run
    /// - Listing feather files in the environment's run folder
    /// - Reading feather files (Arrow IPC stream reading)
    /// - Applying table-only stream conversion transforms
    /// - Writing Arrow batches to the catalog
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_common::enums::Environment;
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
    ///     Environment::Backtest,
    ///     None,
    ///     false,
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn convert_stream_to_data(
        &mut self,
        instance_id: &str,
        data_type: &CatalogDataType,
        environment: Environment,
        identifiers: Option<&[String]>,
        use_ts_event_for_ts_init: bool,
    ) -> anyhow::Result<()> {
        Self::ensure_stream_data_type(data_type)?;
        self.recover_partial_run_files(environment, instance_id);
        let feather_files =
            self.list_feather_files(environment, instance_id, Some(data_type), identifiers)?;

        // Each file is planned before it is written; a file holds every identifier of its type,
        // and each restored batch carries its own identifier
        for file_path in feather_files {
            let batches = Self::filter_batches_by_identifiers(
                self.read_feather_file(&file_path)?,
                &file_path,
                environment,
                instance_id,
                identifiers,
            )?;

            if batches.is_empty() {
                continue;
            }

            self.convert_feather_batches_to_parquet(
                environment,
                instance_id,
                data_type,
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
        environment: Environment,
        instance_id: &str,
        data_type: &CatalogDataType,
        feather_path: &str,
        batches: &[RecordBatch],
        use_ts_event_for_ts_init: bool,
        replay_identity: Option<&str>,
    ) -> anyhow::Result<()> {
        // A type's file holds every identifier, and custom data metadata does not name it, so
        // rows are grouped by identifier as well as by schema
        let mut groups: IndexMap<(Arc<Schema>, Option<String>), Vec<RecordBatch>> = IndexMap::new();

        for batch in batches {
            for (identifier, rows) in split_record_batch_by_identifier(batch)? {
                groups
                    .entry((rows.schema(), identifier))
                    .or_default()
                    .push(rows);
            }
        }

        let mut planned = Vec::new();

        // Number each identifier's groups separately, so selecting identifiers for a conversion
        // does not change the output identity of the groups it keeps.
        let mut ordinals: AHashMap<Option<String>, usize> = AHashMap::new();

        for ((_, identifier), group) in groups {
            let ordinal = ordinals.entry(identifier.clone()).or_default();
            let group_identity = format!(
                "{}/{}/{ordinal}",
                replay_identity.unwrap_or(feather_path),
                identifier.as_deref().unwrap_or_default(),
            );
            *ordinal += 1;

            if let Some(plan) = self.plan_catalog_write(
                environment,
                instance_id,
                data_type,
                feather_path,
                &group,
                use_ts_event_for_ts_init,
                group_identity,
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
        environment: Environment,
        instance_id: &str,
        data_type: &CatalogDataType,
        feather_path: &str,
        group: &[RecordBatch],
        use_ts_event_for_ts_init: bool,
        group_identity: String,
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
            Self::identifier_from_batch_or_path(&batch, feather_path, environment, instance_id);

        let identifier = identifier.as_deref();

        // Like direct catalog writes, promoted files carry the identifier in their directory and
        // metadata
        let batch = record_batch_without_identifier_column(batch)?;

        let directory = match data_type {
            CatalogDataType::Data(NautilusDataType::Instrument) => {
                let class = batch
                    .schema()
                    .metadata()
                    .get(KEY_TYPE_NAME)
                    .ok_or_else(|| anyhow::anyhow!("Staged instrument has no type_name metadata"))?
                    .parse()?;
                self.make_path(instrument_path_prefix(&class), identifier)?
            }
            CatalogDataType::Data(NautilusDataType::Custom { type_name }) => {
                self.make_path_custom_data(type_name, identifier)?
            }
            CatalogDataType::Data(data_type) => {
                self.make_path(&parquet_data_path_prefix(data_type), identifier)?
            }
            CatalogDataType::Record(record_type) => {
                self.make_path(&record_path_prefix(record_type), identifier)?
            }
            CatalogDataType::Instrument(class) => {
                self.make_path(instrument_path_prefix(class), identifier)?
            }
        };

        let batch = Self::with_catalog_identifier_metadata(batch, data_type, identifier)?;

        Ok(Some(PlannedCatalogWrite {
            directory,
            start_ts,
            end_ts,
            batch,
            group_identity,
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
        let mut intervals = existing.clone();
        intervals.extend(remaining.iter().map(|plan| (plan.start_ts, plan.end_ts)));

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!(
                "Writing promoted groups for {directory} would create non-disjoint intervals. \
                 Existing intervals: {existing:?}"
            );
        }

        Ok(remaining)
    }

    fn with_catalog_identifier_metadata(
        batch: RecordBatch,
        data_type: &CatalogDataType,
        identifier: Option<&str>,
    ) -> anyhow::Result<RecordBatch> {
        let Some(identifier) = identifier else {
            return Ok(batch);
        };

        let metadata_key = if *data_type == CatalogDataType::Data(NautilusDataType::Bar) {
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
        environment: Environment,
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

        identifier_from_session_feather_path(feather_path, environment, instance_id)
    }

    fn ensure_stream_data_type(data_type: &CatalogDataType) -> anyhow::Result<()> {
        match data_type {
            // Streams stage instruments under the aggregate family, with the class carried per
            // batch, so a class selector names no staged file
            CatalogDataType::Instrument(class) => anyhow::bail!(
                "Streams stage instruments under the aggregate family, not {class}; \
                 pass the Instrument data type"
            ),
            #[cfg(feature = "defi")]
            CatalogDataType::Data(NautilusDataType::Defi)
            | CatalogDataType::Record(NautilusRecordType::Defi) => {
                anyhow::bail!("Streams do not support {data_type}")
            }
            _ => Ok(()),
        }
    }
}

struct PlannedCatalogWrite {
    directory: String,
    start_ts: u64,
    end_ts: u64,
    batch: RecordBatch,
    group_identity: String,
}

// Splits `batch` into one batch per identifier, with rows that have no identifier as their own
// group; a batch without an identifier column is returned whole
fn split_record_batch_by_identifier(
    batch: &RecordBatch,
) -> anyhow::Result<Vec<(Option<String>, RecordBatch)>> {
    let Ok(identifiers) = identifiers_from_record_batches(std::slice::from_ref(batch)) else {
        return Ok(vec![(None, batch.clone())]);
    };

    let mut split = Vec::with_capacity(identifiers.len() + 1);
    for identifier in identifiers.into_iter().map(Some).chain([None]) {
        if let Some(rows) =
            filter_record_batch_by_identifier(batch, None, |value| value == identifier.as_deref())?
        {
            split.push((identifier, rows));
        }
    }

    Ok(split)
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
    let (start_ts, end_ts) = ParquetDataCatalog::ts_init_range(&batch)?;

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
        left.schema().fields() == right.schema().fields()
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

    let options = Some(SortOptions {
        descending: false,
        nulls_first: false,
    });

    let indices = lexsort_to_indices(
        &[
            SortColumn {
                values: batch.column(ts_init).clone(),
                options,
            },
            SortColumn {
                values: original_row_index,
                options,
            },
        ],
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to sort promotion group: {e}"))?;

    take_record_batch(batch, &indices)
        .map_err(|e| anyhow::anyhow!("Failed to reorder promotion group: {e}"))
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
        // A sliced list keeps its whole child array, so check only the values its rows reference
        DataType::List(_) => array
            .as_any()
            .downcast_ref::<ListArray>()
            .is_none_or(|list| array_has_present_decimal(list_values(list).as_ref())),
        DataType::LargeList(_) => array
            .as_any()
            .downcast_ref::<LargeListArray>()
            .is_none_or(|list| array_has_present_decimal(list_values(list).as_ref())),
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

fn list_values<O: OffsetSizeTrait>(list: &GenericListArray<O>) -> ArrayRef {
    let offsets = list.value_offsets();
    let start = offsets[0].as_usize();
    let end = offsets[offsets.len() - 1].as_usize();
    list.values().slice(start, end - start)
}

#[cfg(test)]
mod promotion_group_tests {
    use std::{collections::HashMap, sync::Arc};

    use datafusion::arrow::{
        array::{Array, Decimal128Array, ListArray, StringArray, UInt64Array},
        buffer::OffsetBuffer,
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_common::enums::Environment;
    use nautilus_model::data::NautilusDataType;
    use nautilus_serialization::arrow::KEY_IDENTIFIER;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::{array_has_present_decimal, split_record_batch_by_identifier};
    use crate::{
        backend::parquet::{catalog::ParquetDataCatalog, io::read_parquet_from_object_store},
        common::storage::create_storage_backend_from_path,
        writer::{
            feather::{NAUTILUS_ARROW_METADATA_ID_COLUMN, NAUTILUS_ARROW_METADATA_JSON_COLUMN},
            run::FeatherSessionSource,
        },
    };

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
    #[case::empty_row(0, false)]
    #[case::populated_row(1, true)]
    fn sliced_list_checks_only_its_rows_for_decimals(#[case] row: usize, #[case] expected: bool) {
        let price = Decimal128Array::from(vec![Some(123_i128)])
            .with_precision_and_scale(38, 16)
            .unwrap();
        let field = Arc::new(Field::new("item", price.data_type().clone(), false));

        let list = ListArray::new(
            field,
            OffsetBuffer::new(vec![0_i32, 0, 1].into()),
            Arc::new(price),
            None,
        );

        assert_eq!(array_has_present_decimal(&list.slice(row, 1)), expected);
    }

    #[rstest]
    #[case::one_identifier(
        vec![Some("A"), None],
        vec![(Some("A"), vec![1]), (None, vec![2])],
    )]
    #[case::two_identifiers(
        vec![Some("A"), None, Some("B"), None],
        vec![(Some("A"), vec![1]), (Some("B"), vec![3]), (None, vec![2, 4])],
    )]
    fn split_by_identifier_keeps_rows_without_identifier_as_their_own_group(
        #[case] identifiers: Vec<Option<&str>>,
        #[case] expected: Vec<(Option<&str>, Vec<u64>)>,
    ) {
        let values = (1..=identifiers.len() as u64).collect::<Vec<_>>();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("value", DataType::UInt64, false),
                Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
            ])),
            vec![
                Arc::new(UInt64Array::from(values)),
                Arc::new(StringArray::from(identifiers)),
            ],
        )
        .unwrap();

        let split = split_record_batch_by_identifier(&batch)
            .unwrap()
            .into_iter()
            .map(|(identifier, rows)| {
                let values = rows
                    .column(0)
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap()
                    .values()
                    .to_vec();
                (identifier, values)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            split,
            expected
                .into_iter()
                .map(|(identifier, values)| (identifier.map(String::from), values))
                .collect::<Vec<_>>(),
        );
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
                Environment::Backtest,
                "run-1",
                &NautilusDataType::QuoteTick.into(),
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap();
        catalog
            .convert_feather_batches_to_parquet(
                Environment::Backtest,
                "run-1",
                &NautilusDataType::QuoteTick.into(),
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
                Environment::Backtest,
                "run-1",
                &NautilusDataType::QuoteTick.into(),
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "overlapping promotion groups have incompatible precision metadata"
        );
        assert!(
            catalog
                .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
                .unwrap()
                .is_empty()
        );
    }

    #[rstest]
    fn overlapping_groups_keep_source_order_for_equal_ts_init() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let batches = vec![
            precision_batch("2", vec![1, 2], Some(10)),
            precision_batch("0", vec![2, 3], None),
        ];

        catalog
            .convert_feather_batches_to_parquet(
                Environment::Backtest,
                "run-1",
                &NautilusDataType::QuoteTick.into(),
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap();

        let files = catalog
            .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
            .unwrap();

        let (written, _) = catalog
            .execute_async(|| async {
                read_parquet_from_object_store(
                    catalog.object_store.clone(),
                    &object_store::path::Path::from(files[0].as_str()),
                )
                .await
            })
            .unwrap();

        let rows = written
            .iter()
            .flat_map(|batch| {
                let ts_init = batch
                    .column_by_name("ts_init")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap()
                    .values()
                    .to_vec();
                let price = batch
                    .column_by_name("price")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Decimal128Array>()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>();
                ts_init.into_iter().zip(price)
            })
            .collect::<Vec<_>>();

        // Coalescing unifies the later group as the left side, so its rows lead on equal ts_init
        assert_eq!(files.len(), 1);
        assert_eq!(
            rows,
            vec![(1, Some(10)), (2, None), (2, Some(10)), (3, None)]
        );
    }

    #[rstest]
    fn promote_feather_file_skips_staged_batches_without_rows() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let storage =
            create_storage_backend_from_path(temp.path().to_str().unwrap(), None).unwrap();
        let source = FeatherSessionSource::new(storage, Environment::Backtest, "run-1");
        let staged = RecordBatch::new_empty(Arc::new(Schema::new(vec![
            Field::new("ts_init", DataType::UInt64, false),
            Field::new(NAUTILUS_ARROW_METADATA_ID_COLUMN, DataType::Utf8, false),
            Field::new(NAUTILUS_ARROW_METADATA_JSON_COLUMN, DataType::Utf8, false),
        ])));

        let summary = catalog
            .promote_feather_file(
                &source,
                "backtest/run-1/quotes_1.feather",
                vec![staged],
                false,
                "replay",
            )
            .unwrap();

        assert!(summary.is_none());
        assert!(
            catalog
                .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
                .unwrap()
                .is_empty()
        );
    }

    #[rstest]
    fn overlapping_groups_with_different_fields_write_nothing() {
        let temp = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp.path(), None, None, None, None);
        let nullable = precision_batch("2", vec![2], Some(2));
        let schema = nullable.schema();
        let non_nullable = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![
                    schema.field(0).clone(),
                    schema.field(1).clone().with_nullable(false),
                ],
                schema.metadata().clone(),
            )),
            nullable.columns().to_vec(),
        )
        .unwrap();
        let batches = vec![precision_batch("2", vec![1, 3], Some(1)), non_nullable];

        let error = catalog
            .convert_feather_batches_to_parquet(
                Environment::Backtest,
                "run-1",
                &NautilusDataType::QuoteTick.into(),
                "backtest/run-1/quotes_1.feather",
                &batches,
                false,
                Some("replay"),
            )
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "overlapping promotion groups have incompatible schemas"
        );
        assert!(
            catalog
                .query_files(&NautilusDataType::QuoteTick.into(), None, None, None)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod stream_folder_tests {
    use nautilus_common::enums::Environment;
    use nautilus_model::data::NautilusDataType;
    use rstest::rstest;

    use crate::{
        catalog::types::CatalogDataType, common::paths::catalog_data_type_from_session_feather_path,
    };

    #[rstest]
    #[case("quotes", NautilusDataType::QuoteTick)]
    #[case("quote_tick", NautilusDataType::QuoteTick)]
    #[case("trades", NautilusDataType::TradeTick)]
    #[case("trade_tick", NautilusDataType::TradeTick)]
    #[case("bars", NautilusDataType::Bar)]
    #[case("bar", NautilusDataType::Bar)]
    #[case("order_book_deltas", NautilusDataType::OrderBookDelta)]
    #[case("order_book_delta", NautilusDataType::OrderBookDelta)]
    #[case("mark_prices", NautilusDataType::MarkPriceUpdate)]
    #[case("mark_price_update", NautilusDataType::MarkPriceUpdate)]
    #[case("index_prices", NautilusDataType::IndexPriceUpdate)]
    #[case("index_price_update", NautilusDataType::IndexPriceUpdate)]
    #[case("funding_rates", NautilusDataType::FundingRateUpdate)]
    #[case("funding_rate_update", NautilusDataType::FundingRateUpdate)]
    #[case("instrument_closes", NautilusDataType::InstrumentClose)]
    #[case("instrument_close", NautilusDataType::InstrumentClose)]
    fn stream_folders_parse_to_their_data_type(
        #[case] folder: &str,
        #[case] expected: NautilusDataType,
    ) {
        let path = format!("backtest/run-1/{folder}/{folder}_0.feather");

        assert_eq!(
            catalog_data_type_from_session_feather_path(&path, Environment::Backtest, "run-1")
                .unwrap(),
            CatalogDataType::from(expected),
        );
    }
}

#[cfg(test)]
mod read_run_tests {
    use std::{
        fs::{self, OpenOptions},
        sync::{Arc, atomic::AtomicU64},
    };

    use nautilus_common::enums::Environment;
    use nautilus_core::UnixNanos;
    use nautilus_model::data::{
        Data, NautilusDataType, QuoteTick, TradeTick,
        stubs::{quote_audusd, quote_ethusdt_binance, stub_trade_ethusdt_buy},
    };
    use rstest::rstest;
    use tempfile::TempDir;

    use crate::{
        backend::parquet::catalog::ParquetDataCatalog,
        catalog::types::CatalogDataType,
        writer::feather::{FEATHER_PARTIAL_EXTENSION, FeatherWriter, RotationConfig, WriterClock},
    };

    fn quote_btc(ts: u64) -> QuoteTick {
        QuoteTick {
            instrument_id: "BTCUSDT-PERP.BINANCE".into(),
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..quote_ethusdt_binance()
        }
    }

    fn quote_aud(ts: u64) -> QuoteTick {
        QuoteTick {
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..quote_audusd()
        }
    }

    fn quote_eth(ts: u64) -> QuoteTick {
        QuoteTick {
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..quote_ethusdt_binance()
        }
    }

    fn trade_eth(ts: u64) -> TradeTick {
        TradeTick {
            ts_event: UnixNanos::from(ts),
            ts_init: UnixNanos::from(ts),
            ..stub_trade_ethusdt_buy()
        }
    }

    fn run_writer(temp_dir: &TempDir) -> FeatherWriter {
        FeatherWriter::new(
            temp_dir.path().join("backtest").join("run-001"),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            None,
        )
    }

    fn read_backtest(temp_dir: &TempDir) -> Vec<Data> {
        ParquetDataCatalog::new(temp_dir.path(), None, None, None, None)
            .read_backtest("run-001")
            .unwrap()
    }

    // Leaves one flushed batch per quote in a partial file whose last batch is cut short, as a
    // writer that crashed mid-append would
    fn write_crashed_run(temp_dir: &TempDir, quotes: &[QuoteTick]) {
        let mut writer = run_writer(temp_dir);

        for quote in quotes {
            writer.write_data(Data::Quote(*quote)).unwrap();
            writer.flush().unwrap();
        }

        writer.close().unwrap();

        let sealed = temp_dir
            .path()
            .join("backtest/run-001/quotes/quotes_0.feather");
        let partial = sealed.with_extension(FEATHER_PARTIAL_EXTENSION);
        fs::rename(&sealed, &partial).unwrap();

        // Drops the end-of-stream marker and the last byte of the final batch
        let file = OpenOptions::new().write(true).open(&partial).unwrap();
        file.set_len(file.metadata().unwrap().len() - 9).unwrap();
    }

    #[rstest]
    fn read_backtest_returns_every_record_sorted_by_ts_init() {
        let temp_dir = TempDir::new().unwrap();
        let mut writer = run_writer(&temp_dir);

        for data in [
            Data::Quote(quote_eth(3_000)),
            Data::Quote(quote_aud(1_000)),
            Data::Trade(trade_eth(2_000)),
            Data::Quote(quote_aud(4_000)),
            Data::Quote(quote_btc(5_000)),
        ] {
            writer.write_data(data).unwrap();
        }

        writer.close().unwrap();

        assert_eq!(
            read_backtest(&temp_dir),
            vec![
                Data::Quote(quote_aud(1_000)),
                Data::Trade(trade_eth(2_000)),
                Data::Quote(quote_eth(3_000)),
                Data::Quote(quote_aud(4_000)),
                Data::Quote(quote_btc(5_000)),
            ],
        );
    }

    #[rstest]
    #[case::selected_identifier_first(true)]
    #[case::every_identifier_first(false)]
    fn convert_stream_to_data_repeats_across_identifier_selections(#[case] selected_first: bool) {
        let temp_dir = TempDir::new().unwrap();
        let mut writer = run_writer(&temp_dir);
        writer.write_data(Data::Quote(quote_aud(1_000))).unwrap();
        writer.write_data(Data::Quote(quote_eth(2_000))).unwrap();
        writer.close().unwrap();

        // The second identifier in the shared file keeps its output identity when selected alone
        let selected = [quote_eth(2_000).instrument_id.to_string()];

        let conversions = if selected_first {
            [Some(&selected[..]), None]
        } else {
            [None, Some(&selected[..])]
        };

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);

        for identifiers in conversions {
            catalog
                .convert_stream_to_data(
                    "run-001",
                    &CatalogDataType::from(NautilusDataType::QuoteTick),
                    Environment::Backtest,
                    identifiers,
                    false,
                )
                .unwrap();
        }

        assert_eq!(
            catalog
                .query::<QuoteTick>(None, None, None, None, None, true)
                .unwrap(),
            vec![quote_aud(1_000), quote_eth(2_000)],
        );
    }

    #[rstest]
    fn read_backtest_skips_open_partial_files() {
        let temp_dir = TempDir::new().unwrap();
        let mut writer = run_writer(&temp_dir);
        writer.write_data(Data::Quote(quote_aud(1_000))).unwrap();
        writer.flush().unwrap();

        let data = read_backtest(&temp_dir);
        writer.close().unwrap();

        assert_eq!(data, Vec::<Data>::new());
        assert_eq!(
            read_backtest(&temp_dir),
            vec![Data::Quote(quote_aud(1_000))]
        );
    }

    #[rstest]
    fn read_backtest_recovers_crashed_partial_files() {
        let temp_dir = TempDir::new().unwrap();
        write_crashed_run(&temp_dir, &[quote_aud(1_000), quote_eth(2_000)]);

        assert_eq!(
            read_backtest(&temp_dir),
            vec![Data::Quote(quote_aud(1_000))]
        );
    }

    #[rstest]
    fn convert_stream_to_data_recovers_crashed_partial_files() {
        let temp_dir = TempDir::new().unwrap();
        write_crashed_run(&temp_dir, &[quote_aud(1_000), quote_eth(2_000)]);
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);

        catalog
            .convert_stream_to_data(
                "run-001",
                &CatalogDataType::from(NautilusDataType::QuoteTick),
                Environment::Backtest,
                None,
                false,
            )
            .unwrap();

        assert_eq!(
            catalog
                .query::<QuoteTick>(None, None, None, None, None, true)
                .unwrap(),
            vec![quote_aud(1_000)],
        );
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn read_backtest_reads_custom_data() {
        use nautilus_model::{
            data::{CustomData, DataType},
            identifiers::InstrumentId,
        };
        use nautilus_serialization::ensure_custom_data_registered;

        use crate::test_data::RustTestCustomData;

        ensure_custom_data_registered::<RustTestCustomData>();
        let instrument_id = InstrumentId::from("RUST.TEST");

        let custom = Data::Custom(CustomData::new(
            Arc::new(RustTestCustomData {
                instrument_id,
                value: 1.23,
                flag: true,
                ts_event: UnixNanos::from(5_000),
                ts_init: UnixNanos::from(5_000),
            }),
            DataType::new("RustTestCustomData", None, Some(instrument_id.to_string())),
        ));

        let temp_dir = TempDir::new().unwrap();
        let mut writer = run_writer(&temp_dir);
        writer.write_data(Data::Quote(quote_aud(1_000))).unwrap();
        writer.write_data(custom.clone()).unwrap();
        writer.close().unwrap();

        assert_eq!(
            read_backtest(&temp_dir),
            vec![Data::Quote(quote_aud(1_000)), custom]
        );
    }
}
