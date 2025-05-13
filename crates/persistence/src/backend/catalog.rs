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

use std::{
    fmt::Debug,
    ops::Bound,
    path::{Path, PathBuf},
};

use datafusion::arrow::record_batch::RecordBatch;
use heck::ToSnakeCase;
use itertools::Itertools;
use log::info;
use nautilus_core::UnixNanos;
use nautilus_model::data::{
    Bar, Data, GetTsInit, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDepth10,
    QuoteTick, TradeTick, close::InstrumentClose,
};
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch},
    parquet::{combine_parquet_files, min_max_from_parquet_metadata, write_batches_to_parquet},
};
use regex::Regex;
use serde::Serialize;
use unbounded_interval_tree::interval_tree::IntervalTree;

use super::session::{self, DataBackendSession, QueryResult, build_query};

pub struct ParquetDataCatalog {
    base_path: PathBuf,
    session: DataBackendSession,
    batch_size: usize,
    compression: parquet::basic::Compression,
    max_row_group_size: usize,
}

impl Debug for ParquetDataCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ParquetDataCatalog))
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl ParquetDataCatalog {
    #[must_use]
    pub fn new(
        base_path: PathBuf,
        batch_size: Option<usize>,
        compression: Option<parquet::basic::Compression>,
        max_row_group_size: Option<usize>,
    ) -> Self {
        let batch_size = batch_size.unwrap_or(5000);
        let compression = compression.unwrap_or(parquet::basic::Compression::SNAPPY);
        let max_row_group_size = max_row_group_size.unwrap_or(5000);

        Self {
            base_path,
            session: session::DataBackendSession::new(batch_size),
            batch_size,
            compression,
            max_row_group_size,
        }
    }

    pub fn write_data_enum(
        &self,
        data: Vec<Data>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) {
        let mut deltas: Vec<OrderBookDelta> = Vec::new();
        let mut depth10s: Vec<OrderBookDepth10> = Vec::new();
        let mut quotes: Vec<QuoteTick> = Vec::new();
        let mut trades: Vec<TradeTick> = Vec::new();
        let mut bars: Vec<Bar> = Vec::new();
        let mut mark_prices: Vec<MarkPriceUpdate> = Vec::new();
        let mut index_prices: Vec<IndexPriceUpdate> = Vec::new();
        let mut closes: Vec<InstrumentClose> = Vec::new();

        for d in data.iter().cloned() {
            match d {
                Data::Deltas(_) => continue,
                Data::Delta(d) => {
                    deltas.push(d);
                }
                Data::Depth10(d) => {
                    depth10s.push(*d);
                }
                Data::Quote(d) => {
                    quotes.push(d);
                }
                Data::Trade(d) => {
                    trades.push(d);
                }
                Data::Bar(d) => {
                    bars.push(d);
                }
                Data::MarkPriceUpdate(p) => {
                    mark_prices.push(p);
                }
                Data::IndexPriceUpdate(p) => {
                    index_prices.push(p);
                }
                Data::InstrumentClose(c) => {
                    closes.push(c);
                }
            }
        }

        // TODO: need to handle instruments here
        let _ = self.write_to_parquet(deltas, start, end);
        let _ = self.write_to_parquet(depth10s, start, end);
        let _ = self.write_to_parquet(quotes, start, end);
        let _ = self.write_to_parquet(trades, start, end);
        let _ = self.write_to_parquet(bars, start, end);
        let _ = self.write_to_parquet(mark_prices, start, end);
        let _ = self.write_to_parquet(index_prices, start, end);
        let _ = self.write_to_parquet(closes, start, end);
    }

    pub fn write_to_parquet<T>(
        &self,
        data: Vec<T>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<PathBuf>
    where
        T: GetTsInit + EncodeToRecordBatch + CatalogPathPrefix,
    {
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let start_ts = start.unwrap_or(data.first().unwrap().ts_init());
        let end_ts = end.unwrap_or(data.last().unwrap().ts_init());

        let batches = self.data_to_record_batches(data)?;
        let schema = batches.first().expect("Batches are empty.").schema();
        let instrument_id = schema.metadata.get("instrument_id").cloned();

        let directory = self.make_path(T::path_prefix(), instrument_id)?;
        let filename = format!("{}-{}.parquet", start_ts.as_u64(), end_ts.as_u64());
        let path = directory.join(&filename);

        // Write all batches to parquet file
        info!(
            "Writing {} batches of {type_name} data to {path:?}",
            batches.len()
        );

        write_batches_to_parquet(
            &batches,
            &path,
            Some(self.compression),
            Some(self.max_row_group_size),
        )?;
        let intervals = self.get_directory_intervals(&directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after writing a new file");
        }

        Ok(path)
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
        if data.is_empty() {
            return Ok(PathBuf::new());
        }

        let type_name = std::any::type_name::<T>().to_snake_case();
        Self::check_ascending_timestamps(&data, &type_name);

        let start_ts = data.first().unwrap().ts_init();
        let end_ts = data.last().unwrap().ts_init();

        let directory = path.unwrap_or(self.make_path(T::path_prefix(), None)?);
        let filename = format!("{}-{}.json", start_ts.as_u64(), end_ts.as_u64());
        let json_path = directory.join(&filename);

        info!(
            "Writing {} records of {type_name} data to {json_path:?}",
            data.len()
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

    /// Extend the timestamp range of an existing parquet file by renaming it
    pub fn extend_file_name(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(data_cls, instrument_id)?;
        let intervals = self.get_directory_intervals(&directory)?;

        let start = start.as_u64();
        let end = end.as_u64();

        for interval in intervals {
            if interval.0 == end + 1 {
                let old_path = directory.join(format!("{}-{}.parquet", interval.0, interval.1));
                let new_path = directory.join(format!("{}-{}.parquet", start, interval.1));
                std::fs::rename(old_path, new_path)?;
                break;
            } else if interval.1 == start - 1 {
                let old_path = directory.join(format!("{}-{}.parquet", interval.0, interval.1));
                let new_path = directory.join(format!("{}-{}.parquet", interval.0, end));
                std::fs::rename(old_path, new_path)?;
                break;
            }
        }

        let intervals = self.get_directory_intervals(&directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after extending a file");
        }

        Ok(())
    }

    pub fn consolidate_catalog(
        &self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.consolidate_directory(&directory, start, end, ensure_contiguous_files)?;
        }

        Ok(())
    }

    pub fn consolidate_data(
        &self,
        type_name: &str,
        instrument_id: Option<String>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(type_name, instrument_id)?;
        self.consolidate_directory(&directory, start, end, ensure_contiguous_files)
    }

    fn consolidate_directory(
        &self,
        directory: &Path,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ensure_contiguous_files: Option<bool>,
    ) -> anyhow::Result<()> {
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

        if parquet_files.len() <= 1 {
            return Ok(());
        }

        let mut files_to_consolidate = Vec::new();
        let mut intervals = Vec::new();
        let start = start.map(|t| t.as_u64());
        let end = end.map(|t| t.as_u64());

        for file in parquet_files {
            if let Some(interval) = parse_filename_timestamps(file.to_str().unwrap()) {
                let (interval_start, interval_end) = interval;
                let include_file = match (start, end) {
                    (Some(s), Some(e)) => interval_start >= s && interval_end <= e,
                    (Some(s), None) => interval_start >= s,
                    (None, Some(e)) => interval_end <= e,
                    (None, None) => true,
                };

                if include_file {
                    files_to_consolidate.push(file);
                    intervals.push(interval);
                }
            }
        }

        intervals.sort_by_key(|&(start, _)| start);

        if !intervals.is_empty() {
            let file_name = format!("{}-{}.parquet", intervals[0].0, intervals.last().unwrap().1);
            let path = directory.join(file_name);
            combine_parquet_files(
                files_to_consolidate.to_vec(),
                &path,
                Some(self.compression),
                Some(self.max_row_group_size),
            )?;
        }

        if ensure_contiguous_files.unwrap_or(true) && !are_intervals_contiguous(&intervals) {
            anyhow::bail!("Intervals are not disjoint after consolidating a directory");
        }

        Ok(())
    }

    /// Reset the filenames of parquet files to match their actual content timestamps
    pub fn reset_catalog_file_names(&self) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.reset_file_names(&directory)?;
        }

        Ok(())
    }

    /// Reset the filenames of parquet files for a specific data type and instrument ID
    pub fn reset_data_file_names(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<()> {
        let directory = self.make_path(data_cls, instrument_id)?;
        self.reset_file_names(&directory)
    }

    /// Reset the filenames of parquet files in a directory
    fn reset_file_names(&self, directory: &Path) -> anyhow::Result<()> {
        if !directory.exists() {
            return Ok(());
        }

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

        for file in parquet_files {
            let (first_ts, last_ts) = min_max_from_parquet_metadata(&file, "ts_init")?;
            let new_filename = format!("{}-{}.parquet", first_ts, last_ts);
            let new_file = directory.join(new_filename);
            std::fs::rename(&file, &new_file)?;
        }

        let intervals = self.get_directory_intervals(directory)?;

        if !are_intervals_disjoint(&intervals) {
            anyhow::bail!("Intervals are not disjoint after resetting file names");
        }

        Ok(())
    }

    pub fn find_leaf_data_directories(&self) -> anyhow::Result<Vec<PathBuf>> {
        let data_dir = self.base_path.join("data");
        let pattern = data_dir.join("**/*").to_string_lossy().to_string();
        let mut leaf_dirs = Vec::new();

        // Get all directories
        let all_dirs: Vec<PathBuf> = glob::glob(&pattern)?
            .filter_map(Result::ok)
            .filter(|p| p.is_dir())
            .collect();

        for dir in all_dirs {
            // Check if directory has any files
            let has_files = glob::glob(&dir.join("*").to_string_lossy())?
                .filter_map(Result::ok)
                .any(|p| p.is_file());

            // Check if directory has any subdirectories
            let has_subdirs = glob::glob(&dir.join("*").to_string_lossy())?
                .filter_map(Result::ok)
                .any(|p| p.is_dir());

            if has_files && !has_subdirs {
                leaf_dirs.push(dir);
            }
        }

        Ok(leaf_dirs)
    }

    /// Query data loaded in the catalog
    pub fn query<T>(
        &mut self,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        where_clause: Option<&str>,
    ) -> anyhow::Result<QueryResult>
    where
        T: DecodeDataFromRecordBatch + CatalogPathPrefix,
    {
        let files_list = self.query_files(T::path_prefix(), instrument_ids, start, end)?;

        for file in files_list {
            let file_str = file.to_str().unwrap();
            let table_name = file
                .file_stem()
                .unwrap()
                .to_str()
                .expect("Failed to convert path to string");

            let query = build_query(table_name, start, end, where_clause);

            self.session
                .add_file::<T>(table_name, file_str, Some(&query))?;
        }

        Ok(self.session.get_query_result())
    }

    /// Query all parquet files for a specific data type and instrument ID
    pub fn query_files(
        &self,
        data_cls: &str,
        instrument_ids: Option<Vec<String>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());

        let safe_ids = instrument_ids.as_ref().map(|ids| {
            ids.iter()
                .map(|id| urisafe_instrument_id(id))
                .collect::<Vec<String>>()
        });

        let base_dir = self.make_path(data_cls, None)?;
        let pattern = base_dir.join("**/*.parquet");
        let pattern_str = pattern.to_string_lossy();

        for entry in glob::glob(&pattern_str)? {
            let path = entry?;
            let path_str = path.to_string_lossy();

            if let Some(ids) = &safe_ids {
                let matches_any_id = ids.iter().any(|safe_id| path_str.contains(safe_id));

                if !matches_any_id {
                    continue;
                }
            }

            if query_intersects_filename(&path_str, start_u64, end_u64) {
                files.push(path);
            }
        }

        Ok(files)
    }

    /// Find the missing time intervals for a specific data type and instrument ID
    pub fn get_missing_intervals_for_request(
        &self,
        start: u64,
        end: u64,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let intervals = self.get_intervals(data_cls, instrument_id)?;

        Ok(query_interval_diff(start, end, &intervals))
    }

    /// Get the last timestamp for a specific data type and instrument ID
    pub fn query_last_timestamp(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Option<u64>> {
        let intervals = self.get_intervals(data_cls, instrument_id)?;

        if intervals.is_empty() {
            return Ok(None);
        }

        Ok(Some(intervals.last().unwrap().1))
    }

    /// Get the time intervals covered by parquet files for a specific data type and instrument ID
    pub fn get_intervals(
        &self,
        data_cls: &str,
        instrument_id: Option<String>,
    ) -> anyhow::Result<Vec<(u64, u64)>> {
        let directory = self.make_path(data_cls, instrument_id)?;

        self.get_directory_intervals(&directory)
    }

    /// Get the time intervals covered by parquet files in a directory
    fn get_directory_intervals(&self, directory: &Path) -> anyhow::Result<Vec<(u64, u64)>> {
        let mut intervals = Vec::new();

        if directory.exists() {
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();

                if path.is_file() && path.extension().unwrap() == "parquet" {
                    if let Some(interval) = parse_filename_timestamps(path.to_str().unwrap()) {
                        intervals.push(interval);
                    }
                }
            }
        }

        intervals.sort_by_key(|&(start, _)| start);

        Ok(intervals)
    }

    /// Create a directory path for a data type and instrument ID
    fn make_path(&self, type_name: &str, instrument_id: Option<String>) -> anyhow::Result<PathBuf> {
        let mut path = self.base_path.join("data").join(type_name);

        if let Some(id) = instrument_id {
            path = path.join(urisafe_instrument_id(&id));
        }

        Ok(path)
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
impl_catalog_path_prefix!(IndexPriceUpdate, "index_prices");
impl_catalog_path_prefix!(MarkPriceUpdate, "mark_prices");
impl_catalog_path_prefix!(InstrumentClose, "instrument_closes");

// Helper functions for interval operations

fn urisafe_instrument_id(instrument_id: &str) -> String {
    instrument_id.replace("/", "")
}

/// Check if a filename intersects with a query interval
fn query_intersects_filename(filename: &str, start: Option<u64>, end: Option<u64>) -> bool {
    if let Some((file_start, file_end)) = parse_filename_timestamps(filename) {
        (start.is_none() || start.unwrap() <= file_end)
            && (end.is_none() || file_start <= end.unwrap())
    } else {
        true
    }
}

/// Parse timestamps from a filename in the format "start-end.parquet"
fn parse_filename_timestamps(filename: &str) -> Option<(u64, u64)> {
    let re = Regex::new(r"(\d+)-(\d+)\.parquet$").unwrap();
    let path = Path::new(filename);
    let base_name = path.file_name()?.to_str()?;

    re.captures(base_name).map(|caps| {
        let first_ts = caps.get(1).unwrap().as_str().parse::<u64>().unwrap();
        let last_ts = caps.get(2).unwrap().as_str().parse::<u64>().unwrap();
        (first_ts, last_ts)
    })
}

/// Checks if a list of closed integer intervals are all mutually disjoint.
fn are_intervals_disjoint(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();

    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 >= start2 {
            return false;
        }
    }

    true
}

/// Intervals are contiguous if, when sorted, each interval's start is exactly one more than the previous interval's end.
fn are_intervals_contiguous(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();
    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 + 1 != start2 {
            return false;
        }
    }

    true
}

/// Finds the parts of the interval [start, end] (inclusive) that are not covered by the 'closed_intervals'.
fn query_interval_diff(start: u64, end: u64, closed_intervals: &[(u64, u64)]) -> Vec<(u64, u64)> {
    if start > end {
        return Vec::new();
    }

    let interval_set = get_interval_set(closed_intervals);
    let query_range = (Bound::Included(start), Bound::Included(end));
    let query_diff = interval_set.get_interval_difference(&query_range);
    let mut result: Vec<(u64, u64)> = Vec::new();

    for interval in query_diff {
        if let Some(tuple) = interval_to_tuple(interval, start, end) {
            result.push(tuple);
        }
    }

    result
}

/// Creates an IntervalTree where each closed integer interval (a,b) is represented as a half-open interval [a, b+1).
fn get_interval_set(intervals: &[(u64, u64)]) -> IntervalTree<u64> {
    let mut tree = IntervalTree::default();

    if intervals.is_empty() {
        return tree;
    }

    for &(start, end) in intervals {
        if start > end {
            continue;
        }

        tree.insert((
            Bound::Included(start),
            Bound::Excluded(end.saturating_add(1)),
        ));
    }

    tree
}

fn interval_to_tuple(
    interval: (Bound<&u64>, Bound<&u64>),
    query_start: u64,
    query_end: u64,
) -> Option<(u64, u64)> {
    let (bound_start, bound_end) = interval;

    let start = match bound_start {
        Bound::Included(val) => *val,
        Bound::Excluded(val) => val.saturating_add(1),
        Bound::Unbounded => query_start,
    };

    let end = match bound_end {
        Bound::Included(val) => *val,
        Bound::Excluded(val) => {
            if *val == 0 {
                return None; // Empty interval
            }
            val - 1
        }
        Bound::Unbounded => query_end,
    };

    if start <= end {
        Some((start, end))
    } else {
        None
    }
}
