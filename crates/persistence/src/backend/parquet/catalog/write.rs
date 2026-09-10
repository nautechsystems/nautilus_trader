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

//! Parquet catalog write paths.

#![expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "catalog write functions validate catalog-controlled batches and forward storage errors"
)]

use nautilus_serialization::arrow::catalog_identifier_from_metadata;

use super::{
    BTreeMap, CatalogDataType, CustomData, Data, DataBatch, EncodeToRecordBatch, HasTsInit,
    InstrumentAny, NautilusRecordType, ObjectPath, ObjectStoreExt, Params, ParquetDataCatalog,
    PathBuf, RecordBatch, Serialize, UnixNanos, WRITE_SKIP_DISJOINT_CHECK, are_intervals_disjoint,
    instrument_any_type, instrument_path_prefix, parquet_data_path_prefix,
    prepare_custom_data_batch, record_batch_without_identifier_column, record_path_prefix,
    timestamps_to_filename, to_snake_case, write_batches_to_object_store, write_catalog_batch,
};
use crate::{
    backend::parquet::io::write_batches_to_object_store_create,
    common::metadata::record_batch_ts_init_range,
};

impl ParquetDataCatalog {
    /// Writes mixed data types to the catalog by separating them into type-specific collections.
    ///
    /// This method takes a heterogeneous collection of market data and separates it by type,
    /// then writes each type to its appropriate location in the catalog. This is useful when
    /// processing mixed data streams or bulk data imports.
    ///
    /// # Parameters
    ///
    /// - `data`: A vector of mixed [`Data`] enum variants.
    /// - `start`: Optional start timestamp to override the data's natural range.
    /// - `end`: Optional end timestamp to override the data's natural range.
    ///
    /// # Notes
    ///
    /// - Data is automatically sorted by type before writing.
    /// - Each data type is written to its own directory structure.
    /// - Instrument data handling is not yet implemented (TODO).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::Data;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let mixed_data: Vec<Data> = vec![/* mixed data types */];
    ///
    /// catalog.write_data_enum(&mixed_data, None, None, None)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_data_enum(
        &self,
        data: &[Data],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<()> {
        for batch in DataBatch::from_data_vec_grouped(data)? {
            write_catalog_batch(self, &batch, start, end, skip_disjoint_check)?;
        }
        Ok(())
    }

    pub(super) fn write_grouped_to_parquet<T>(
        &self,
        data: &[T],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<()>
    where
        T: Clone + HasTsInit + EncodeToRecordBatch + CatalogDataType,
    {
        let mut groups: BTreeMap<Option<String>, Vec<T>> = BTreeMap::new();

        for item in data {
            let identifier = catalog_identifier_from_metadata(&item.metadata());
            groups.entry(identifier).or_default().push(item.clone());
        }

        for items in groups.into_values() {
            self.write_to_parquet(&items, start, end, skip_disjoint_check)?;
        }
        Ok(())
    }

    /// Writes Arrow batches into catalog under a record type optional identifier.
    ///
    /// # Errors
    ///
    /// Returns error if batches do not contain `ts_init` or cannot be persisted.
    pub fn write_record_batches(
        &mut self,
        record_type: &NautilusRecordType,
        identifier: Option<&str>,
        batches: &[RecordBatch],
        params: &Params,
    ) -> anyhow::Result<()> {
        if batches.is_empty() || batches.iter().all(|batch| batch.num_rows() == 0) {
            return Ok(());
        }

        let (start_ts, end_ts) = record_batch_ts_init_range(batches)?;
        let record_prefix = record_path_prefix(record_type);
        let directory = self.make_path(record_prefix.as_ref(), identifier)?;
        let filename = timestamps_to_filename(UnixNanos::from(start_ts), UnixNanos::from(end_ts));
        let path = PathBuf::from(directory.clone()).join(&filename);
        let object_path = self.to_object_path(&path.to_string_lossy())?;
        let skip_disjoint_check = params.get_bool(WRITE_SKIP_DISJOINT_CHECK).unwrap_or(false);

        if !skip_disjoint_check {
            let current_intervals = self.get_directory_intervals(&directory)?;
            let mut intervals = current_intervals.clone();
            intervals.push((start_ts, end_ts));
            anyhow::ensure!(
                are_intervals_disjoint(&intervals),
                "Writing file {filename} interval ({start_ts}, {end_ts}) would create non-disjoint intervals. Existing intervals: {current_intervals:?}",
            );
        }

        self.execute_async(|| async {
            write_batches_to_object_store(
                batches,
                self.object_store.clone(),
                &object_path,
                Some(self.compression),
                Some(self.max_row_group_size),
                None,
            )
            .await
        })
    }

    /// Writes typed data to a Parquet file in the catalog.
    ///
    /// This is the core method for persisting market data to the catalog. It handles data
    /// validation, batching, compression, and ensures proper file organization with
    /// timestamp-based naming.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement required traits for serialization and cataloging.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order).
    /// - `start`: Optional start timestamp to override the natural data range.
    /// - `end`: Optional end timestamp to override the natural data range.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created file, or an empty path if no data was provided.
    /// If the target file already exists, returns the path without writing (skips write).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization to Arrow record batches fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    /// - Writing would create non-disjoint timestamp intervals.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Data timestamps are not in ascending order.
    /// - Record batches are empty after conversion.
    /// - Required metadata is missing from the schema.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::data::QuoteTick;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let quotes: Vec<QuoteTick> = vec![/* quote data */];
    ///
    /// let path = catalog.write_to_parquet(&quotes, None, None, None)?;
    /// println!("Data written to: {:?}", path);
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_to_parquet<T>(
        &self,
        data: &[T],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<PathBuf>
    where
        T: HasTsInit + EncodeToRecordBatch + CatalogDataType,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = to_snake_case(std::any::type_name::<T>());
        Self::check_ascending_timestamps(data, &type_name)?;

        let chunk_metadata = T::chunk_metadata(data);
        if let Some(position) = data
            .iter()
            .position(|item| !item.matches_chunk_metadata(&chunk_metadata))
        {
            anyhow::bail!(
                "Cannot write {type_name} data with mixed identities: element {position} has \
                 metadata {:?} but the chunk has {chunk_metadata:?}; write each \
                 instrument or bar type separately",
                data[position].metadata(),
            );
        }

        let start_ts = start.unwrap_or(data.first().unwrap().ts_init());
        let end_ts = end.unwrap_or(data.last().unwrap().ts_init());

        let batches = self.data_to_record_batches(data)?;
        let schema = batches.first().expect("Batches are empty.").schema();

        let data_type = T::catalog_data_type();
        let path_prefix = parquet_data_path_prefix(&data_type);
        let identifier = if matches!(data_type, super::NautilusDataType::Bar) {
            schema.metadata.get("bar_type").cloned()
        } else {
            schema.metadata.get("instrument_id").cloned()
        };

        let directory = self.make_path(path_prefix.as_ref(), identifier.as_deref())?;
        self.write_parquet_file_checked(
            &directory,
            start_ts,
            end_ts,
            &batches,
            skip_disjoint_check.unwrap_or(false),
            "File",
            Some(&format!("{type_name} data")),
            None,
        )
    }

    /// Writes custom data to a Parquet file in the catalog.
    ///
    /// This method handles writing custom data types that implement `CustomDataTrait`.
    /// Custom data is organized by type name in a `custom/{type_name}/` directory structure.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of custom data items to write (must be in ascending timestamp order).
    /// - `start`: Optional start timestamp to override the natural data range.
    /// - `end`: Optional end timestamp to override the natural data range.
    /// - `skip_disjoint_check`: Whether to skip interval disjointness validation.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created file, or an empty path if no data was provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization to Arrow record batches fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    /// - Writing would create non-disjoint timestamp intervals (unless skipped).
    pub fn write_custom_data_batch<D>(
        &self,
        data: D,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<PathBuf>
    where
        D: AsRef<[CustomData]>,
    {
        let data = data.as_ref();
        let data = data.iter().collect::<Vec<_>>();
        self.write_custom_data_refs_batch(&data, start, end, skip_disjoint_check)
    }

    pub(crate) fn write_custom_data_refs_batch(
        &self,
        data: &[&CustomData],
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        skip_disjoint_check: Option<bool>,
    ) -> anyhow::Result<PathBuf> {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let (batch, type_name, identifier, start_ts, end_ts) = prepare_custom_data_batch(data)?;
        let start_ts = start.unwrap_or(start_ts);
        let end_ts = end.unwrap_or(end_ts);
        let batches = vec![record_batch_without_identifier_column(batch)?];

        let directory = self.make_path_custom_data(&type_name, identifier.as_deref())?;
        self.write_parquet_file_checked(
            &directory,
            start_ts,
            end_ts,
            &batches,
            skip_disjoint_check.unwrap_or(false),
            "File",
            None,
            None,
        )
    }

    /// Writes instruments to Parquet files in the catalog.
    ///
    /// Instruments are stored under their instrument ID directory using timestamp-ranged
    /// file names, allowing multiple historical versions of the same instrument to be
    /// appended over time:
    /// `data/instruments/{instrument_id}/{start_ts}-{end_ts}.parquet`
    ///
    /// # Parameters
    ///
    /// - `instruments`: Vector of instruments to write.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to the created files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Data serialization fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_model::instruments::InstrumentAny;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let instruments: Vec<InstrumentAny> = vec![/* instruments */];
    ///
    /// let paths = catalog.write_instruments(instruments)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_instruments(
        &self,
        instruments: Vec<InstrumentAny>,
    ) -> anyhow::Result<Vec<PathBuf>> {
        use nautilus_model::instruments::Instrument;

        if instruments.is_empty() {
            return Ok(Vec::new());
        }

        // Group instruments by concrete type and instrument_id so mixed InstrumentAny
        // inputs are written as separate parquet batches with stable ordering.
        let mut by_type_and_id: BTreeMap<(String, String), Vec<InstrumentAny>> = BTreeMap::new();

        for instrument in instruments {
            let instrument_type = instrument_any_type(&instrument);
            let instrument_prefix = instrument_path_prefix(&instrument_type).to_string();
            let instrument_id = Instrument::id(&instrument).to_string();
            by_type_and_id
                .entry((instrument_prefix, instrument_id))
                .or_default()
                .push(instrument);
        }

        let mut paths = Vec::new();

        for ((instrument_prefix, instrument_id), instrument_group) in by_type_and_id {
            Self::check_ascending_timestamps(&instrument_group, "instrument")?;

            let start_ts = HasTsInit::ts_init(instrument_group.first().unwrap());
            let end_ts = HasTsInit::ts_init(instrument_group.last().unwrap());
            let batches = self.data_to_record_batches(&instrument_group)?;
            if batches.is_empty() {
                continue;
            }

            let directory = self.make_path(&instrument_prefix, Some(instrument_id.as_str()))?;

            // ArrowWriter stores the full schema (including "class" metadata) in ARROW:schema.
            // When reading, use the builder's schema for metadata (see query_instruments).
            let path = self.write_parquet_file_checked(
                &directory,
                start_ts,
                end_ts,
                &batches,
                false,
                "Instrument file",
                Some(&format!("instrument data for {instrument_id}")),
                None,
            )?;

            paths.push(path);
        }

        Ok(paths)
    }

    /// Writes `batches` to a timestamp-named parquet file in `directory`, skipping when the
    /// target file already exists and validating interval disjointness unless `skip_disjoint_check`.
    ///
    /// `file_label` and `data_description` parameterize the log messages so callers keep their
    /// site-specific wording (`data_description = None` suppresses the pre-write log).
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn write_parquet_file_checked(
        &self,
        directory: &str,
        start_ts: UnixNanos,
        end_ts: UnixNanos,
        batches: &[RecordBatch],
        skip_disjoint_check: bool,
        file_label: &str,
        data_description: Option<&str>,
        replay_identity: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        let filename = timestamps_to_filename(start_ts, end_ts);
        let filename = replay_identity.map_or(filename.clone(), |identity| {
            let stem = filename.strip_suffix(".parquet").unwrap_or(&filename);
            let digest = blake3::hash(identity.as_bytes()).to_hex();
            format!("{stem}_{}.parquet", &digest[..16])
        });
        let path = PathBuf::from(directory).join(&filename);
        let object_path = self.to_object_path(&path.to_string_lossy())?;

        let file_exists = self.execute_async(|| async {
            let exists: bool = self.object_store.head(&object_path).await.is_ok();
            Ok(exists)
        })?;

        if file_exists {
            log::info!(
                "{file_label} {} already exists, skipping write",
                path.display()
            );
            return Ok(path);
        }

        if !skip_disjoint_check {
            let current_intervals = self.get_directory_intervals(directory)?;
            let new_interval = (start_ts.as_u64(), end_ts.as_u64());
            let mut new_intervals = current_intervals.clone();
            new_intervals.push(new_interval);

            if !are_intervals_disjoint(&new_intervals) {
                anyhow::bail!(
                    "Writing file {filename} with interval ({start_ts}, {end_ts}) would create \
                    non-disjoint intervals. Existing intervals: {current_intervals:?}"
                );
            }
        }

        if let Some(data_description) = data_description {
            log::info!(
                "Writing {} batches of {data_description} to {}",
                batches.len(),
                path.display(),
            );
        }

        self.execute_async(|| async {
            let result = if replay_identity.is_some() {
                write_batches_to_object_store_create(
                    batches,
                    self.object_store.clone(),
                    &object_path,
                    Some(self.compression),
                    Some(self.max_row_group_size),
                    None,
                )
                .await
            } else {
                write_batches_to_object_store(
                    batches,
                    self.object_store.clone(),
                    &object_path,
                    Some(self.compression),
                    Some(self.max_row_group_size),
                    None,
                )
                .await
            };

            if let Err(e) = result {
                if replay_identity.is_some()
                    && matches!(
                        e.downcast_ref::<object_store::Error>(),
                        Some(object_store::Error::AlreadyExists { .. })
                    )
                {
                    return Ok(());
                }
                return Err(e);
            }
            Ok(())
        })?;

        Ok(path)
    }

    /// Writes typed data to a JSON file in the catalog.
    ///
    /// This method provides an alternative to Parquet format for data export and debugging.
    /// JSON files are human-readable but less efficient for large datasets.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to write, must implement serialization and cataloging traits.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to write (must be in ascending timestamp order).
    /// - `path`: Optional custom directory path (defaults to catalog's standard structure).
    /// - `write_metadata`: Whether to write a separate metadata file alongside the data.
    ///
    /// # Returns
    ///
    /// Returns the [`PathBuf`] of the created JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - JSON serialization fails.
    /// - Object store write operations fail.
    /// - File path construction fails.
    ///
    /// # Panics
    ///
    /// Panics if data timestamps are not in ascending order.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::path::PathBuf;
    /// use nautilus_model::data::TradeTick;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let trades: Vec<TradeTick> = vec![/* trade data */];
    ///
    /// let path = catalog.write_to_json(
    ///     trades,
    ///     Some(PathBuf::from("/custom/path")),
    ///     true  // write metadata
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn write_to_json<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        write_metadata: bool,
    ) -> anyhow::Result<PathBuf>
    where
        T: HasTsInit + Serialize + CatalogDataType + EncodeToRecordBatch,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = to_snake_case(std::any::type_name::<T>());
        Self::check_ascending_timestamps(&data, &type_name)?;

        let start_ts = data.first().unwrap().ts_init();
        let end_ts = data.last().unwrap().ts_init();

        let data_type = T::catalog_data_type();
        let path_prefix = parquet_data_path_prefix(&data_type);
        let directory = path
            .unwrap_or_else(|| PathBuf::from(self.make_path(path_prefix.as_ref(), None).unwrap()));
        let filename = timestamps_to_filename(start_ts, end_ts).replace(".parquet", ".json");
        let json_path = directory.join(&filename);

        log::info!(
            "Writing {} records of {type_name} data to {}",
            data.len(),
            json_path.display(),
        );

        if write_metadata {
            let metadata = T::chunk_metadata(&data);
            let metadata_path = json_path.with_extension("metadata.json");
            log::info!("Writing metadata to {}", metadata_path.display());

            // Use object store for metadata file
            let metadata_object_path = ObjectPath::from(metadata_path.to_string_lossy().as_ref());
            let metadata_json = serde_json::to_vec_pretty(&metadata)?;
            self.execute_async(|| async {
                let _: object_store::PutResult = self
                    .object_store
                    .put(&metadata_object_path, metadata_json.into())
                    .await?;
                Ok(())
            })?;
        }

        // Use object store for main JSON file
        let json_object_path = ObjectPath::from(json_path.to_string_lossy().as_ref());
        let json_data = serde_json::to_vec_pretty(&serde_json::to_value(data)?)?;
        self.execute_async(|| async {
            let _: object_store::PutResult = self
                .object_store
                .put(&json_object_path, json_data.into())
                .await?;
            Ok(())
        })?;

        Ok(json_path)
    }

    /// Validates that data timestamps are in ascending order.
    ///
    /// # Parameters
    ///
    /// - `data`: Slice of data records to validate.
    /// - `type_name`: Name of the data type for error messages.
    pub fn check_ascending_timestamps<T: HasTsInit>(
        data: &[T],
        type_name: &str,
    ) -> anyhow::Result<()> {
        if !data
            .array_windows()
            .all(|[a, b]| a.ts_init() <= b.ts_init())
        {
            anyhow::bail!("{type_name} timestamps must be in ascending order");
        }

        Ok(())
    }

    /// Converts data into Arrow record batches for Parquet serialization.
    ///
    /// This method chunks the data according to the configured batch size and converts
    /// each chunk into an Arrow record batch with appropriate metadata.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The data type to convert, must implement required encoding traits.
    ///
    /// # Parameters
    ///
    /// - `data`: Vector of data records to convert.
    ///
    /// # Returns
    ///
    /// Returns a vector of Arrow [`RecordBatch`] instances ready for Parquet serialization.
    ///
    /// # Errors
    ///
    /// Returns an error if record batch encoding fails for any chunk.
    pub fn data_to_record_batches<T>(&self, data: &[T]) -> anyhow::Result<Vec<RecordBatch>>
    where
        T: HasTsInit + EncodeToRecordBatch,
    {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let mut batches = Vec::new();
        let metadata = EncodeToRecordBatch::chunk_metadata(data);

        for chunk in data.chunks(self.batch_size) {
            let record_batch = T::encode_batch(&metadata, chunk)?;
            let record_batch = record_batch_without_identifier_column(record_batch)?;
            batches.push(record_batch);
        }

        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::Display,
        fs::File,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use arrow::{
        array::Int64Array,
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use futures::stream::BoxStream;
    use nautilus_core::UnixNanos;
    use nautilus_model::data::{
        OrderBookDelta, OrderBookDepth,
        stubs::{stub_delta, stub_depth10},
    };
    use nautilus_serialization::arrow::{
        DecodeFromRecordBatch, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
    };
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
        ObjectStoreExt, PutMode, PutMultipartOptions, PutOptions, PutPayload, PutResult,
        Result as ObjectStoreResult, memory::InMemory, path::Path as ObjectPath,
    };
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use rstest::rstest;
    use tempfile::TempDir;

    use super::ParquetDataCatalog;
    use crate::common::datafusion::DataBackendSession;

    #[rstest]
    fn depth_write_shares_file_metadata_across_chunks(stub_depth10: OrderBookDepth) {
        let mut empty = stub_depth10.clone();
        empty.bids.clear();
        empty.asks.clear();
        empty.bid_counts.clear();
        empty.ask_counts.clear();
        let directory = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            Some(2),
            None,
            Some(2),
        )
        .unwrap();
        let data = vec![empty.clone(), empty, stub_depth10];

        let path = catalog.write_to_parquet(&data, None, None, None).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(
            File::open(directory.path().join(path)).unwrap(),
        )
        .unwrap();
        let metadata = builder.schema().metadata().clone();
        let batches = builder
            .build()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let decoded = batches
            .iter()
            .cloned()
            .flat_map(|batch| OrderBookDepth::decode_batch(&metadata, batch).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(metadata[KEY_PRICE_PRECISION], "2");
        assert_eq!(metadata[KEY_SIZE_PRECISION], "0");
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded, data);
        assert_eq!(decoded[2].bids[0].price.precision, 2);
    }

    #[rstest]
    fn leading_clear_delta_writes_with_following_precision(stub_delta: OrderBookDelta) {
        let directory = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::from_uri(
            directory.path().to_str().unwrap(),
            None,
            Some(2),
            None,
            None,
        )
        .unwrap();
        let clear = OrderBookDelta::clear(
            stub_delta.instrument_id,
            0,
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let second_clear = OrderBookDelta::clear(
            stub_delta.instrument_id,
            0,
            UnixNanos::from(2),
            UnixNanos::from(2),
        );

        let path = catalog
            .write_to_parquet(&[clear, second_clear, stub_delta], None, None, None)
            .unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(
            File::open(directory.path().join(&path)).unwrap(),
        )
        .unwrap();
        let metadata = builder.schema().metadata().clone();
        let decoded = builder
            .build()
            .unwrap()
            .map(|batch| OrderBookDelta::decode_batch(&metadata, batch.unwrap()).unwrap())
            .collect::<Vec<_>>()
            .concat();

        assert!(directory.path().join(path).exists());
        assert_eq!(metadata[KEY_PRICE_PRECISION], "2");
        assert_eq!(decoded[2].order.price.precision, 2);
    }

    #[derive(Debug)]
    struct CreateRaceStore {
        inner: InMemory,
        create_calls: AtomicUsize,
    }

    impl Display for CreateRaceStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("create-race")
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for CreateRaceStore {
        async fn put_opts(
            &self,
            location: &ObjectPath,
            payload: PutPayload,
            opts: PutOptions,
        ) -> ObjectStoreResult<PutResult> {
            if opts.mode == PutMode::Create {
                self.create_calls.fetch_add(1, Ordering::Relaxed);
                return Err(object_store::Error::AlreadyExists {
                    path: location.to_string(),
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "injected create race",
                    )),
                });
            }
            self.inner.put_opts(location, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjectPath,
            opts: PutMultipartOptions,
        ) -> ObjectStoreResult<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, opts).await
        }

        async fn get_opts(
            &self,
            location: &ObjectPath,
            options: GetOptions,
        ) -> ObjectStoreResult<GetResult> {
            self.inner.get_opts(location, options).await
        }

        fn list(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> ObjectStoreResult<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, ObjectStoreResult<ObjectPath>>,
        ) -> BoxStream<'static, ObjectStoreResult<ObjectPath>> {
            self.inner.delete_stream(locations)
        }

        async fn copy_opts(
            &self,
            from: &ObjectPath,
            to: &ObjectPath,
            opts: CopyOptions,
        ) -> ObjectStoreResult<()> {
            self.inner.copy_opts(from, to, opts).await
        }
    }

    #[rstest]
    fn promotion_writes_to_memory_store_without_copy_support() {
        let catalog = ParquetDataCatalog {
            base_path: "catalog".to_string(),
            original_uri: "memory://".to_string(),
            object_store: Arc::new(InMemory::new()),
            session: DataBackendSession::new(5_000),
            batch_size: 5_000,
            compression: parquet::basic::Compression::SNAPPY,
            max_row_group_size: 5_000,
        };
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "ts_init",
                DataType::Int64,
                false,
            )])),
            vec![Arc::new(Int64Array::from(vec![1]))],
        )
        .unwrap();

        let path = catalog
            .write_parquet_file_checked(
                "quotes/TEST",
                UnixNanos::from(1),
                UnixNanos::from(1),
                &[batch],
                false,
                "Promoted file",
                None,
                Some("memory-replay"),
            )
            .unwrap();
        let object_path = catalog.to_object_path(&path.to_string_lossy()).unwrap();
        catalog
            .execute_async(|| async {
                catalog.object_store.head(&object_path).await?;
                Ok(())
            })
            .unwrap();
    }

    #[rstest]
    fn promotion_accepts_already_exists_after_head_miss() {
        let object_store = Arc::new(CreateRaceStore {
            inner: InMemory::new(),
            create_calls: AtomicUsize::new(0),
        });
        let catalog = ParquetDataCatalog {
            base_path: "catalog".to_string(),
            original_uri: "memory://".to_string(),
            object_store: object_store.clone(),
            session: DataBackendSession::new(5_000),
            batch_size: 5_000,
            compression: parquet::basic::Compression::SNAPPY,
            max_row_group_size: 5_000,
        };
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "ts_init",
                DataType::Int64,
                false,
            )])),
            vec![Arc::new(Int64Array::from(vec![1]))],
        )
        .unwrap();

        catalog
            .write_parquet_file_checked(
                "quotes/TEST",
                UnixNanos::from(1),
                UnixNanos::from(1),
                &[batch],
                true,
                "Promoted file",
                None,
                Some("racing-replay"),
            )
            .unwrap();

        assert_eq!(object_store.create_calls.load(Ordering::Relaxed), 1);
    }
}
