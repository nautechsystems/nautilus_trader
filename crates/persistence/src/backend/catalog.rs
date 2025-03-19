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

use std::path::PathBuf;

use datafusion::arrow::record_batch::RecordBatch;
use heck::ToSnakeCase;
use itertools::Itertools;
use log::info;
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, Data, GetTsInit, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
};
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch},
    enums::ParquetWriteMode,
    parquet::{combine_data_files, min_max_from_parquet_metadata, write_batches_to_parquet},
};
use serde::Serialize;

use super::session::{self, DataBackendSession, QueryResult, build_query};

pub struct ParquetDataCatalog {
    base_path: PathBuf,
    batch_size: usize,
    session: DataBackendSession,
}

impl ParquetDataCatalog {
    #[must_use]
    pub fn new(base_path: PathBuf, batch_size: Option<usize>) -> Self {
        let batch_size = batch_size.unwrap_or(5000);
        Self {
            base_path,
            batch_size,
            session: session::DataBackendSession::new(batch_size),
        }
    }

    pub fn write_data_enum(&self, data: Vec<Data>, write_mode: Option<ParquetWriteMode>) {
        let mut delta: Vec<OrderBookDelta> = Vec::new();
        let mut depth10: Vec<OrderBookDepth10> = Vec::new();
        let mut quote: Vec<QuoteTick> = Vec::new();
        let mut trade: Vec<TradeTick> = Vec::new();
        let mut bar: Vec<Bar> = Vec::new();

        for d in data.iter().cloned() {
            match d {
                Data::Delta(d) => {
                    delta.push(d);
                }
                Data::Depth10(d) => {
                    depth10.push(*d);
                }
                Data::Quote(d) => {
                    quote.push(d);
                }
                Data::Trade(d) => {
                    trade.push(d);
                }
                Data::Bar(d) => {
                    bar.push(d);
                }
                Data::Deltas(_) => continue,
            }
        }

        let _ = self.write_to_parquet(delta, None, None, None, write_mode);
        let _ = self.write_to_parquet(depth10, None, None, None, write_mode);
        let _ = self.write_to_parquet(quote, None, None, None, write_mode);
        let _ = self.write_to_parquet(trade, None, None, None, write_mode);
        let _ = self.write_to_parquet(bar, None, None, None, write_mode);
    }

    pub fn write_to_parquet<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
        write_mode: Option<ParquetWriteMode>,
    ) -> anyhow::Result<PathBuf>
    where
        T: GetTsInit + EncodeToRecordBatch + CatalogPathPrefix,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);
        let batches = self.data_to_record_batches(data)?;
        let schema = batches.first().expect("Batches are empty.").schema();
        let instrument_id = schema.metadata.get("instrument_id").cloned();
        let new_path = self.make_path(T::path_prefix(), instrument_id, write_mode)?;
        let path = path.unwrap_or(new_path);

        // Write all batches to parquet file
        info!(
            "Writing {} batches of {type_name} data to {path:?}",
            batches.len()
        );

        write_batches_to_parquet(&batches, &path, compression, max_row_group_size, write_mode)?;

        Ok(path)
    }

    fn check_ascending_timestamps<T: GetTsInit>(data: &[T], type_name: &str) {
        assert!(
            data.windows(2).all(|w| w[0].ts_init() <= w[1].ts_init()),
            "{type_name} timestamps must be in ascending order"
        );
    }

    pub fn data_to_record_batches<T>(&self, data: Vec<T>) -> anyhow::Result<Vec<RecordBatch>>
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        let mut batches = Vec::new();

        for chunk in &data.into_iter().chunks(self.batch_size) {
            let data = chunk.collect_vec();
            let metadata = EncodeToRecordBatch::chunk_metadata(&data);
            let record_batch = T::encode_batch(&metadata, &data)?;
            batches.push(record_batch);
        }

        Ok(batches)
    }

    fn make_path(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        write_mode: Option<ParquetWriteMode>,
    ) -> anyhow::Result<PathBuf> {
        let path = self.make_directory_path(type_name, instrument_id);
        std::fs::create_dir_all(&path)?;
        let used_write_mode = write_mode.unwrap_or(ParquetWriteMode::Overwrite);
        let mut file_path = path.join("data-0.parquet");
        let mut empty_path = file_path.clone();
        let mut i = 0;

        while empty_path.exists() {
            i += 1;
            let name = format!("data-{i}.parquet");
            empty_path = path.join(name);
        }

        if i > 1 && used_write_mode != ParquetWriteMode::NewFile {
            anyhow::bail!(
                "Only ParquetWriteMode::NewFile is allowed for a directory containing several parquet files."
            );
        } else if used_write_mode == ParquetWriteMode::NewFile {
            file_path = empty_path;
        }

        info!("Created directory path: {file_path:?}");

        Ok(file_path)
    }

    fn make_directory_path(&self, type_name: &str, instrument_id: Option<String>) -> PathBuf {
        let mut path = self.base_path.join("data").join(type_name);

        if let Some(id) = instrument_id {
            path = path.join(id.replace('/', "")); // for FX symbols like EUR/USD
        }

        path
    }

    pub fn write_to_json<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        write_metadata: bool,
    ) -> anyhow::Result<PathBuf>
    where
        T: GetTsInit + Serialize + CatalogPathPrefix + EncodeToRecordBatch,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);
        let new_path = self.make_path(T::path_prefix(), None, None)?;
        let json_path = path.unwrap_or(new_path.with_extension("json"));

        info!(
            "Writing {} records of {type_name} data to {json_path:?}",
            data.len(),
        );

        if write_metadata {
            let metadata = T::chunk_metadata(&data);
            let metadata_path = json_path.with_extension("metadata.json");
            info!("Writing metadata to {metadata_path:?}");
            let metadata_file = std::fs::File::create(&metadata_path)?;
            serde_json::to_writer_pretty(metadata_file, &metadata)?;
        }

        let file = std::fs::File::create(&json_path)?;
        serde_json::to_writer_pretty(file, &serde_json::to_value(data)?)?;

        Ok(json_path)
    }

    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<()> {
        let parquet_files = self.query_parquet_files(type_name, instrument_id)?;

        if !parquet_files.is_empty() {
            combine_data_files(parquet_files, "ts_init", None, None)?;
        }

        Ok(())
    }

    pub fn consolidate_catalog(&self) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            let parquet_files: Vec<PathBuf> = std::fs::read_dir(directory)?
                .filter_map(|entry| {
                    let path = entry.ok()?.path();

                    if path.extension().and_then(|s| s.to_str()) == Some("parquet") {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();

            if !parquet_files.is_empty() {
                combine_data_files(parquet_files, "ts_init", None, None)?;
            }
        }

        Ok(())
    }

    pub fn find_leaf_data_directories(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut all_paths: Vec<PathBuf> = Vec::new();
        let data_dir = self.base_path.join("data");

        for entry in walkdir::WalkDir::new(data_dir) {
            all_paths.push(entry?.path().to_path_buf());
        }

        let all_dirs = all_paths
            .iter()
            .filter(|p| p.is_dir())
            .cloned()
            .collect::<Vec<PathBuf>>();
        let mut leaf_dirs = Vec::new();

        for directory in all_dirs {
            let items = std::fs::read_dir(&directory)?;
            let has_subdirs = items.into_iter().any(|entry| {
                let entry = entry.unwrap();
                entry.path().is_dir()
            });
            let has_files = std::fs::read_dir(&directory)?.any(|entry| {
                let entry = entry.unwrap();
                entry.path().is_file()
            });

            if has_files && !has_subdirs {
                leaf_dirs.push(directory);
            }
        }

        Ok(leaf_dirs)
    }

    /// Query data loaded in the catalog
    pub fn query_file<T>(
        &mut self,
        path: PathBuf,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        let path_str = path.to_str().expect("Failed to convert path to string");
        let table_name = path
            .file_stem()
            .unwrap()
            .to_str()
            .expect("Failed to convert path to string");
        let query = build_query(table_name, start, end, where_clause);
        self.session
            .add_file::<T>(table_name, path_str, Some(&query))?;

        Ok(self.session.get_query_result())
    }

    /// Query data loaded in the catalog
    pub fn query_directory<T>(
        &mut self,
        instrument_ids: Vec<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        let mut paths = Vec::new();

        for instrument_id in instrument_ids {
            paths.extend(self.query_parquet_files(T::path_prefix(), Some(instrument_id))?);
        }

        // If no specific instrument_id is selected query all files for the data type
        if paths.is_empty() {
            paths.push(self.make_path(T::path_prefix(), None, None)?);
        }

        for path in &paths {
            let path = path.to_str().expect("Failed to convert path to string");
            let query = build_query(path, start, end, where_clause);
            self.session.add_file::<T>(path, path, Some(&query))?;
        }

        Ok(self.session.get_query_result())
    }

    #[allow(dead_code)]
    pub fn query_timestamp_bound(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        is_last: Option<bool>,
    ) -> anyhow::Result<Option<i64>> {
        let is_last = is_last.unwrap_or(true);
        let parquet_files = self.query_parquet_files(data_cls, instrument_id)?;

        if parquet_files.is_empty() {
            return Ok(None);
        }

        let min_max_per_file: Vec<(i64, i64)> = parquet_files
            .iter()
            .map(|file| min_max_from_parquet_metadata(file, "ts_init"))
            .collect::<Result<Vec<_>, _>>()?;
        let mut timestamps: Vec<i64> = Vec::new();

        for min_max in min_max_per_file {
            let (min, max) = min_max;

            if is_last {
                timestamps.push(max);
            } else {
                timestamps.push(min);
            }
        }

        if timestamps.is_empty() {
            return Ok(None);
        }

        if is_last {
            Ok(timestamps.iter().max().copied())
        } else {
            Ok(timestamps.iter().min().copied())
        }
    }

    pub fn query_parquet_files(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let path = self.make_directory_path(type_name, instrument_id);
        let mut files = Vec::new();

        if path.exists() {
            for entry in std::fs::read_dir(path)? {
                let path = entry?.path();
                if path.is_file() && path.extension().unwrap() == "parquet" {
                    files.push(path);
                }
            }
        }

        Ok(files)
    }
}

pub trait CatalogPathPrefix {
    fn path_prefix() -> &'static str;
}

macro_rules! impl_catalog_path_prefix {
    ($type:ty, $path:expr) => {
        impl CatalogPathPrefix for $type {
            fn path_prefix() -> &'static str {
                $path
            }
        }
    };
}

impl_catalog_path_prefix!(QuoteTick, "quotes");
impl_catalog_path_prefix!(TradeTick, "trades");
impl_catalog_path_prefix!(OrderBookDelta, "order_book_deltas");
impl_catalog_path_prefix!(OrderBookDepth10, "order_book_depths");
impl_catalog_path_prefix!(Bar, "bars");
