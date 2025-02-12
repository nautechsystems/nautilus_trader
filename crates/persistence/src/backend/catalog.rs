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

use datafusion::{arrow::record_batch::RecordBatch, error::Result};
use heck::ToSnakeCase;
use itertools::Itertools;
use log::info;
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, Data, GetTsInit, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
};
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch},
    parquet::write_batches_to_parquet,
};
use serde::Serialize;

use super::session::{self, build_query, DataBackendSession, QueryResult};

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

    fn make_path(&self, type_name: &str, instrument_id: Option<&String>) -> PathBuf {
        let mut path = self.base_path.join("data").join(type_name);

        if let Some(id) = instrument_id {
            path = path.join(id);
        }

        std::fs::create_dir_all(&path).expect("Failed to create directory");
        let file_path = path.join("data.parquet");
        info!("Created directory path: {:?}", file_path);
        file_path
    }

    fn check_ascending_timestamps<T: GetTsInit>(data: &[T], type_name: &str) {
        assert!(
            data.windows(2).all(|w| w[0].ts_init() <= w[1].ts_init()),
            "{type_name} timestamps must be in ascending order"
        );
    }

    #[must_use]
    pub fn data_to_record_batches<T>(&self, data: Vec<T>) -> Vec<RecordBatch>
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        data.into_iter()
            .chunks(self.batch_size)
            .into_iter()
            .map(|chunk| {
                // Take first element and extract metadata
                // SAFETY: Unwrap safe as already checked that `data` not empty
                let data = chunk.collect_vec();
                let metadata = EncodeToRecordBatch::chunk_metadata(&data);
                T::encode_batch(&metadata, &data).expect("Expected to encode batch")
            })
            .collect()
    }

    #[must_use]
    pub fn write_to_json<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        write_metadata: bool,
    ) -> PathBuf
    where
        T: GetTsInit + Serialize + CatalogPathPrefix + EncodeToRecordBatch,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let json_path = path.unwrap_or_else(|| {
            let path = self.make_path(T::path_prefix(), None);
            path.with_extension("json")
        });

        info!(
            "Writing {} records of {type_name} data to {json_path:?}",
            data.len(),
        );

        if write_metadata {
            let metadata = T::chunk_metadata(&data);
            let metadata_path = json_path.with_extension("metadata.json");
            info!("Writing metadata to {:?}", metadata_path);
            let metadata_file = std::fs::File::create(&metadata_path)
                .unwrap_or_else(|_| panic!("Failed to create metadata file at {metadata_path:?}"));
            serde_json::to_writer_pretty(metadata_file, &metadata)
                .unwrap_or_else(|_| panic!("Failed to write metadata to JSON"));
        }

        let file = std::fs::File::create(&json_path)
            .unwrap_or_else(|_| panic!("Failed to create JSON file at {json_path:?}"));

        serde_json::to_writer_pretty(file, &serde_json::to_value(data).unwrap())
            .unwrap_or_else(|_| panic!("Failed to write {type_name} to JSON"));

        json_path
    }

    #[must_use]
    pub fn write_to_parquet<T>(
        &self,
        data: Vec<T>,
        path: Option<PathBuf>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> PathBuf
    where
        T: GetTsInit + EncodeToRecordBatch + CatalogPathPrefix,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let batches = self.data_to_record_batches(data);
        let batch = batches.first().expect("Expected at least one batch");
        let schema = batch.schema();
        let instrument_id = schema.metadata.get("instrument_id");
        let path = path.unwrap_or_else(|| self.make_path(T::path_prefix(), instrument_id));

        // Write all batches to parquet file
        info!(
            "Writing {} batches of {} data to {:?}",
            batches.len(),
            type_name,
            path
        );

        write_batches_to_parquet(&batches, &path, compression, max_row_group_size)
            .unwrap_or_else(|_| panic!("Failed to write {type_name} to parquet"));

        path
    }

    /// Query data loaded in the catalog
    pub fn query_file<T>(
        &mut self,
        path: PathBuf,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        let path_str = path.to_str().unwrap();
        let table_name = path.file_stem().unwrap().to_str().unwrap();
        let query = build_query(table_name, start, end, where_clause);
        self.session
            .add_file::<T>(table_name, path_str, Some(&query))?;
        Ok(self.session.get_query_result())
    }

    /// Query data loaded in the catalog
    pub fn query_directory<T>(
        &mut self,
        // use instrument_ids or bar_types to query specific subset of the data
        instrument_ids: Vec<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        let mut paths = Vec::new();
        for instrument_id in &instrument_ids {
            paths.push(self.make_path(T::path_prefix(), Some(instrument_id)));
        }

        // If no specific instrument_id is selected query all files for the data type
        if paths.is_empty() {
            paths.push(self.make_path(T::path_prefix(), None));
        }

        for path in &paths {
            let path = path.to_str().unwrap();
            let query = build_query(path, start, end, where_clause);
            self.session.add_file::<T>(path, path, Some(&query))?;
        }

        Ok(self.session.get_query_result())
    }

    pub fn write_data_enum(&self, data: Vec<Data>) {
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

        let _ = self.write_to_parquet(delta, None, None, None);
        let _ = self.write_to_parquet(depth10, None, None, None);
        let _ = self.write_to_parquet(quote, None, None, None);
        let _ = self.write_to_parquet(trade, None, None, None);
        let _ = self.write_to_parquet(bar, None, None, None);
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
