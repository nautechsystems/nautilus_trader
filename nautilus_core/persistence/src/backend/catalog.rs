// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_core::nanos::UnixNanos;
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

    // TODO: fix path creation
    fn make_path(&self, type_name: &str, instrument_id: Option<&String>) -> PathBuf {
        let mut path = self.base_path.join("data").join(type_name.to_lowercase());

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
    pub fn write_to_json<T>(&self, data: Vec<T>) -> PathBuf
    where
        T: GetTsInit + Serialize,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let path = self.make_path(&type_name, None);
        let json_path = path.with_extension("json");

        info!(
            "Writing {} records of {} data to {:?}",
            data.len(),
            type_name,
            json_path
        );

        let file = std::fs::File::create(&json_path)
            .unwrap_or_else(|_| panic!("Failed to create JSON file at {json_path:?}"));

        serde_json::to_writer(file, &data)
            .unwrap_or_else(|_| panic!("Failed to write {type_name} to JSON"));

        json_path
    }

    pub fn write_to_parquet<T>(&self, data: Vec<T>)
    where
        T: GetTsInit + EncodeToRecordBatch,
    {
        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let batches = self.data_to_record_batches(data);
        if let Some(batch) = batches.first() {
            let schema = batch.schema();
            let instrument_id = schema.metadata.get("instrument_id");
            let path = self.make_path(&type_name, instrument_id);

            // Write all batches to parquet file
            info!(
                "Writing {} batches of {} data to {:?}",
                batches.len(),
                type_name,
                path
            );
            // TODO: Set writer to property to limit max row group size
            write_batches_to_parquet(&batches, &path, None, Some(5000))
                .unwrap_or_else(|_| panic!("Failed to write {type_name} to parquet"));
        }
    }

    /// Query data loaded in the catalog
    pub fn query<T>(
        &mut self,
        // use instrument_ids or bar_types to query specific subset of the data
        instrument_ids: Vec<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch,
    {
        let mut paths = Vec::new();
        for instrument_id in &instrument_ids {
            paths.push(self.make_path("TODO", Some(instrument_id)));
        }

        // If no specific instrument_id is selected query all files for the data type
        if paths.is_empty() {
            paths.push(self.make_path("TODO", None));
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
                    depth10.push(d);
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

        self.write_to_parquet(delta);
        self.write_to_parquet(depth10);
        self.write_to_parquet(quote);
        self.write_to_parquet(trade);
        self.write_to_parquet(bar);
    }
}
