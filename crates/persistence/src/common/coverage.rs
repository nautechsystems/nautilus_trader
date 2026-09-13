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

//! Closed nanosecond interval coverage for catalog data, empty, and deleted segments.

use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use arrow::{
    array::{Array, Int64Array, StringArray, UInt32Array, UInt64Array},
    compute::take_record_batch,
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_core::ClosedInterval;
use nautilus_serialization::arrow::{StringColumnRef, U32ColumnRef, U64ColumnRef};

use super::{
    CREATED_TS_COLUMN, DATA_TYPE_COLUMN, DATA_VERSION_COLUMN, END_TS_COLUMN, ROW_COUNT_COLUMN,
    SCHEMA_VERSION_COLUMN, SOURCE_COLUMN, START_TS_COLUMN, STATUS_COLUMN, TABLE_PATH_COLUMN,
};

pub const COVERAGE_SCHEMA_VERSION: u32 = 1;

/// What a coverage segment records in the catalog log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageKind {
    Data,
    Empty,
    Deleted,
}

/// A data, empty, or deleted segment recorded for a catalog key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoverageSegment {
    pub interval: ClosedInterval,
    pub kind: CoverageKind,
}

/// Effective data and known-empty coverage intervals for one request key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoverageIntervals {
    /// Intervals backed by stored data.
    pub data: Vec<ClosedInterval>,
    /// Intervals explicitly known to contain no data.
    pub empty: Vec<ClosedInterval>,
}

/// Durable coverage row shared by table-backed catalog implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCoverageRow {
    pub table_path: String,
    pub data_type: String,
    pub identifier: Option<String>,
    pub start_ts: u64,
    pub end_ts: u64,
    pub kind: CoverageKind,
    pub row_count: u64,
    pub data_version: Option<i64>,
    pub source: String,
    pub created_ts: u64,
    pub schema_version: u32,
}

/// Converts a durable row to a closed segment.
///
/// Returns `None` for an invalid inverted timestamp range instead of normalizing it.
impl From<&CatalogCoverageRow> for Option<CoverageSegment> {
    fn from(row: &CatalogCoverageRow) -> Self {
        CoverageSegment::new(row.start_ts, row.end_ts, row.kind)
    }
}

/// Converts valid durable coverage rows to closed segments, skipping inverted ranges.
pub fn rows_to_segments<'a, I>(rows: I) -> Vec<CoverageSegment>
where
    I: IntoIterator<Item = &'a CatalogCoverageRow>,
{
    rows.into_iter().filter_map(Into::into).collect()
}

/// Returns the latest timestamp still covered by data after applying deletions.
///
/// Empty coverage rows do not represent stored data and are skipped.
#[must_use]
pub fn last_data_timestamp(rows: &[CatalogCoverageRow]) -> Option<u64> {
    let mut intervals = Vec::new();
    for row in rows {
        let Some(interval) = ClosedInterval::new(row.start_ts, row.end_ts) else {
            continue;
        };

        match row.kind {
            CoverageKind::Data => intervals.push(interval),
            CoverageKind::Deleted => {
                intervals = subtract_interval_from_intervals(&intervals, interval);
            }
            CoverageKind::Empty => {}
        }
    }
    merge_closed_intervals(&intervals)
        .into_iter()
        .map(|interval| interval.end)
        .max()
}

pub fn deduplicate_and_sort_coverage_rows(
    rows: Vec<CatalogCoverageRow>,
) -> Vec<CatalogCoverageRow> {
    let mut rows = deduplicate_coverage_rows(rows);
    rows.sort_by(compare_coverage_rows_causal);
    rows
}

#[must_use]
pub fn deduplicate_coverage_rows(rows: Vec<CatalogCoverageRow>) -> Vec<CatalogCoverageRow> {
    let mut deduplicated = BTreeMap::new();

    for row in rows {
        let key = (
            row.table_path.clone(),
            row.data_type.clone(),
            row.identifier.clone(),
            row.start_ts,
            row.end_ts,
            coverage_kind_rank(row.kind),
            row.row_count,
        );

        if let Some(existing) = deduplicated.get_mut(&key) {
            if compare_coverage_rows_causal(&row, existing).is_gt() {
                *existing = row;
            }
        } else {
            deduplicated.insert(key, row);
        }
    }

    deduplicated.into_values().collect()
}

/// # Errors
///
/// Returns an error if `ts_init` is missing, has an unsupported type, contains invalid values,
/// or the selected rows cannot be materialized.
pub fn filter_record_batch_by_ts_init_intervals(
    batch: &RecordBatch,
    intervals: &[ClosedInterval],
) -> anyhow::Result<Option<(RecordBatch, Vec<u64>)>> {
    let ts_init = batch
        .column_by_name("ts_init")
        .ok_or_else(|| anyhow::anyhow!("ts_init column not found"))?;
    let values = U64ColumnRef::try_from_array(ts_init.as_ref())
        .ok_or_else(|| anyhow::anyhow!("ts_init column must be UInt64 or Int64"))?;
    let mut indices = Vec::with_capacity(batch.num_rows());
    let mut timestamps = Vec::with_capacity(batch.num_rows());

    for row in 0..batch.num_rows() {
        anyhow::ensure!(!ts_init.is_null(row), "ts_init column contains null values");
        let timestamp = values
            .value(row)
            .ok_or_else(|| anyhow::anyhow!("ts_init value cannot be negative"))?;

        if intervals
            .iter()
            .any(|interval| interval.start <= timestamp && timestamp <= interval.end)
        {
            indices.push(u32::try_from(row)?);
            timestamps.push(timestamp);
        }
    }

    if indices.is_empty() {
        return Ok(None);
    }

    if indices.len() == batch.num_rows() {
        return Ok(Some((batch.clone(), timestamps)));
    }

    let indices = UInt32Array::from(indices);
    let batch = take_record_batch(batch, &indices)
        .map_err(|e| anyhow::anyhow!("Failed to filter batch by coverage: {e}"))?;
    Ok(Some((batch, timestamps)))
}

/// Orders coverage rows causally. `data_version` is a monotonic ordering token derived
/// from the writer's base snapshot, not a snapshot handle; rows written before versioning
/// existed carry `None` and order as version 0, which is correct because every versioned
/// row postdates them. Ties (including compacted rows sharing one version) fall back to
/// the creation timestamp, keeping the order total and transitive on mixed catalogs.
#[must_use]
pub fn compare_coverage_rows_causal(
    left: &CatalogCoverageRow,
    right: &CatalogCoverageRow,
) -> Ordering {
    left.data_version
        .unwrap_or(0)
        .cmp(&right.data_version.unwrap_or(0))
        .then(left.created_ts.cmp(&right.created_ts))
        .then(left.start_ts.cmp(&right.start_ts))
        .then(left.end_ts.cmp(&right.end_ts))
}

const fn coverage_kind_rank(kind: CoverageKind) -> u8 {
    match kind {
        CoverageKind::Data => 0,
        CoverageKind::Empty => 1,
        CoverageKind::Deleted => 2,
    }
}

#[must_use]
pub fn coverage_kind_to_str(kind: CoverageKind) -> &'static str {
    match kind {
        CoverageKind::Data => "data",
        CoverageKind::Empty => "empty",
        CoverageKind::Deleted => "deleted",
    }
}

fn coverage_kind_from_str(value: &str) -> anyhow::Result<CoverageKind> {
    match value {
        "data" => Ok(CoverageKind::Data),
        "empty" => Ok(CoverageKind::Empty),
        "deleted" => Ok(CoverageKind::Deleted),
        other => anyhow::bail!("Unknown catalog coverage status: {other}"),
    }
}

#[must_use]
pub fn coverage_batch_schema() -> Schema {
    Schema::new(vec![
        Field::new(TABLE_PATH_COLUMN, DataType::Utf8, false),
        Field::new(DATA_TYPE_COLUMN, DataType::Utf8, false),
        Field::new("identifier", DataType::Utf8, true),
        Field::new(START_TS_COLUMN, DataType::UInt64, false),
        Field::new(END_TS_COLUMN, DataType::UInt64, false),
        Field::new(STATUS_COLUMN, DataType::Utf8, false),
        Field::new(ROW_COUNT_COLUMN, DataType::UInt64, false),
        Field::new(DATA_VERSION_COLUMN, DataType::Int64, true),
        Field::new(SOURCE_COLUMN, DataType::Utf8, false),
        Field::new(CREATED_TS_COLUMN, DataType::UInt64, false),
        Field::new(SCHEMA_VERSION_COLUMN, DataType::UInt32, false),
    ])
}

/// # Errors
///
/// Returns an error if the coverage rows cannot be encoded with the canonical Arrow schema.
pub fn coverage_rows_to_batch(rows: &[CatalogCoverageRow]) -> anyhow::Result<RecordBatch> {
    Ok(RecordBatch::try_new(
        Arc::new(coverage_batch_schema()),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.table_path.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.data_type.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.identifier.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.start_ts).collect::<Vec<_>>(),
            )),
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.end_ts).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| coverage_kind_to_str(row.kind))
                    .collect::<Vec<_>>(),
            )),
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.row_count).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter().map(|row| row.data_version).collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.source.clone())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.created_ts).collect::<Vec<_>>(),
            )),
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|row| row.schema_version)
                    .collect::<Vec<_>>(),
            )),
        ],
    )?)
}

/// # Errors
///
/// Returns an error if a batch does not match the canonical coverage schema or contains an
/// unknown coverage status.
pub fn decode_coverage_batches(
    batches: Vec<RecordBatch>,
) -> anyhow::Result<Vec<CatalogCoverageRow>> {
    let mut rows = Vec::new();

    for batch in batches {
        let table_path = string_values(&batch, TABLE_PATH_COLUMN)?;
        let data_type = string_values(&batch, DATA_TYPE_COLUMN)?;
        let identifier = string_values(&batch, "identifier")?;
        let status = string_values(&batch, STATUS_COLUMN)?;
        let source = string_values(&batch, SOURCE_COLUMN)?;
        let start_ts = u64_values(&batch, START_TS_COLUMN)?;
        let end_ts = u64_values(&batch, END_TS_COLUMN)?;
        let row_count = u64_values(&batch, ROW_COUNT_COLUMN)?;
        let created_ts = u64_values(&batch, CREATED_TS_COLUMN)?;
        let data_version = batch
            .column_by_name(DATA_VERSION_COLUMN)
            .ok_or_else(|| anyhow::anyhow!("{DATA_VERSION_COLUMN} column not found"))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| anyhow::anyhow!("{DATA_VERSION_COLUMN} column is not Int64"))?;
        let schema_version = u32_values(&batch, SCHEMA_VERSION_COLUMN)?;

        for row in 0..batch.num_rows() {
            rows.push(CatalogCoverageRow {
                table_path: table_path[row]
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("{TABLE_PATH_COLUMN} is null"))?,
                data_type: data_type[row]
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("{DATA_TYPE_COLUMN} is null"))?,
                identifier: identifier[row].clone(),
                start_ts: start_ts[row],
                end_ts: end_ts[row],
                kind: coverage_kind_from_str(
                    status[row]
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("{STATUS_COLUMN} is null"))?,
                )?,
                row_count: row_count[row],
                data_version: (!data_version.is_null(row)).then(|| data_version.value(row)),
                source: source[row]
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("{SOURCE_COLUMN} is null"))?,
                created_ts: created_ts[row],
                schema_version: schema_version[row],
            });
        }
    }

    Ok(rows)
}

fn u64_values(batch: &RecordBatch, name: &str) -> anyhow::Result<Vec<u64>> {
    let column = batch
        .column_by_name(name)
        .ok_or_else(|| anyhow::anyhow!("{name} column not found"))?;
    let values = U64ColumnRef::try_from_array(column.as_ref())
        .ok_or_else(|| anyhow::anyhow!("{name} column is not UInt64 or Int64"))?;

    (0..values.len())
        .map(|row| {
            anyhow::ensure!(!values.is_null(row), "{name} column contains null values");
            values.value(row).ok_or_else(|| {
                anyhow::anyhow!("Catalog coverage integer column contains a negative value")
            })
        })
        .collect()
}

fn u32_values(batch: &RecordBatch, name: &str) -> anyhow::Result<Vec<u32>> {
    let column = batch
        .column_by_name(name)
        .ok_or_else(|| anyhow::anyhow!("{name} column not found"))?;
    let values = U32ColumnRef::try_from_array(column.as_ref())
        .ok_or_else(|| anyhow::anyhow!("{name} column is not UInt32, Int32, or Int64"))?;

    match values {
        U32ColumnRef::UInt32(values) => {
            anyhow::ensure!(
                values.null_count() == 0,
                "{name} column contains null values"
            );
            Ok(values.values().to_vec())
        }
        U32ColumnRef::Int32(values) => {
            anyhow::ensure!(
                values.null_count() == 0,
                "{name} column contains null values"
            );
            values
                .values()
                .iter()
                .map(|value| {
                    u32::try_from(*value).map_err(|_| {
                        anyhow::anyhow!("Catalog coverage integer column contains a negative value")
                    })
                })
                .collect()
        }
        U32ColumnRef::Int64(values) => {
            anyhow::ensure!(
                values.null_count() == 0,
                "{name} column contains null values"
            );
            values
                .values()
                .iter()
                .map(|value| {
                    u32::try_from(*value).map_err(|_| {
                        anyhow::anyhow!(
                            "Catalog coverage integer column contains a value outside the u32 range"
                        )
                    })
                })
                .collect()
        }
    }
}

fn string_values(batch: &RecordBatch, name: &str) -> anyhow::Result<Vec<Option<String>>> {
    let column = batch
        .column_by_name(name)
        .ok_or_else(|| anyhow::anyhow!("{name} column not found"))?;
    let values = StringColumnRef::try_from_array(column.as_ref())
        .ok_or_else(|| anyhow::anyhow!("{name} column is not Utf8 or Utf8View"))?;
    Ok((0..batch.num_rows())
        .map(|row| values.value_opt(row).map(str::to_string))
        .collect())
}

impl CoverageSegment {
    /// Creates a segment if `start <= end`.
    #[must_use]
    pub const fn new(start: u64, end: u64, kind: CoverageKind) -> Option<Self> {
        match ClosedInterval::new(start, end) {
            Some(interval) => Some(Self { interval, kind }),
            None => None,
        }
    }
}

/// Merges closed intervals, treating adjacent intervals as contiguous coverage.
#[must_use]
pub fn merge_closed_intervals(intervals: &[ClosedInterval]) -> Vec<ClosedInterval> {
    let mut intervals: Vec<ClosedInterval> = intervals
        .iter()
        .copied()
        .filter(|interval| interval.start <= interval.end)
        .collect();

    if intervals.is_empty() {
        return Vec::new();
    }

    intervals.sort_by_key(|interval| interval.start);

    let mut merged = Vec::with_capacity(intervals.len());
    let mut current = intervals[0];

    for interval in intervals.into_iter().skip(1) {
        if interval.start <= current.end.saturating_add(1) {
            current.end = current.end.max(interval.end);
        } else {
            merged.push(current);
            current = interval;
        }
    }

    merged.push(current);
    merged
}

/// Returns the effective union of all data and empty segments after applying delete tombstones.
#[must_use]
pub fn covered_intervals(segments: &[CoverageSegment]) -> Vec<ClosedInterval> {
    let mut intervals = Vec::new();

    for segment in segments {
        match segment.kind {
            CoverageKind::Data | CoverageKind::Empty => {
                intervals.push(segment.interval);
            }
            CoverageKind::Deleted => {
                intervals = merge_closed_intervals(&intervals);
                intervals = subtract_interval_from_merged_intervals(&intervals, segment.interval);
            }
        }
    }

    merge_closed_intervals(&intervals)
}

/// Returns effective data and known-empty intervals after applying delete tombstones.
#[must_use]
pub fn coverage_intervals_by_kind(segments: &[CoverageSegment]) -> CoverageIntervals {
    let mut data = Vec::new();
    let mut empty = Vec::new();

    for segment in segments {
        match segment.kind {
            CoverageKind::Data => data.push(segment.interval),
            CoverageKind::Empty => empty.push(segment.interval),
            CoverageKind::Deleted => {
                data = subtract_interval_from_merged_intervals(
                    &merge_closed_intervals(&data),
                    segment.interval,
                );
                empty = subtract_interval_from_merged_intervals(
                    &merge_closed_intervals(&empty),
                    segment.interval,
                );
            }
        }
    }

    let data = merge_closed_intervals(&data);
    let mut empty = merge_closed_intervals(&empty);
    for interval in &data {
        empty = subtract_interval_from_merged_intervals(&empty, *interval);
    }

    CoverageIntervals { data, empty }
}

/// Returns the portions of `[request_start, request_end]` not covered by data or empty segments.
#[must_use]
pub fn missing_segments_for_request(
    request_start: u64,
    request_end: u64,
    segments: &[CoverageSegment],
) -> Vec<ClosedInterval> {
    let covered = covered_intervals(segments);
    missing_intervals(request_start, request_end, &covered)
}

/// Subtracts `deleted` from already merged coverage intervals.
#[must_use]
pub fn subtract_interval_from_intervals(
    intervals: &[ClosedInterval],
    deleted: ClosedInterval,
) -> Vec<ClosedInterval> {
    subtract_interval_from_merged_intervals(&merge_closed_intervals(intervals), deleted)
}

fn subtract_interval_from_merged_intervals(
    intervals: &[ClosedInterval],
    deleted: ClosedInterval,
) -> Vec<ClosedInterval> {
    let mut remaining = Vec::new();

    for interval in intervals.iter().copied() {
        if deleted.end < interval.start || deleted.start > interval.end {
            remaining.push(interval);
            continue;
        }

        if deleted.start > interval.start {
            remaining.push(ClosedInterval {
                start: interval.start,
                end: deleted.start.saturating_sub(1),
            });
        }

        if deleted.end < interval.end {
            remaining.push(ClosedInterval {
                start: deleted.end.saturating_add(1),
                end: interval.end,
            });
        }
    }

    remaining
}

/// Returns the portions of `[request_start, request_end]` not covered by closed intervals.
#[must_use]
pub fn missing_intervals(
    request_start: u64,
    request_end: u64,
    intervals: &[ClosedInterval],
) -> Vec<ClosedInterval> {
    if request_start > request_end {
        return Vec::new();
    }

    let intervals = merge_closed_intervals(intervals);
    let mut missing = Vec::new();
    let mut cursor = request_start;

    for interval in intervals {
        if interval.end < cursor {
            continue;
        }

        if interval.start > request_end {
            break;
        }

        if cursor < interval.start {
            missing.push(ClosedInterval {
                start: cursor,
                end: interval.start - 1,
            });
        }

        if interval.end >= request_end {
            return missing;
        }

        cursor = interval.end.saturating_add(1);
    }

    if cursor <= request_end {
        missing.push(ClosedInterval {
            start: cursor,
            end: request_end,
        });
    }

    missing
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn merge_closed_intervals_treats_adjacent_nanoseconds_as_contiguous() {
        let intervals = [
            ClosedInterval::new(20, 30).unwrap(),
            ClosedInterval::new(10, 19).unwrap(),
            ClosedInterval::new(50, 55).unwrap(),
        ];

        assert_eq!(
            merge_closed_intervals(&intervals),
            vec![
                ClosedInterval::new(10, 30).unwrap(),
                ClosedInterval::new(50, 55).unwrap(),
            ],
        );
    }

    #[rstest]
    fn missing_segments_for_request_treats_empty_segments_as_covered() {
        let segments = [
            CoverageSegment::new(1, 4, CoverageKind::Empty).unwrap(),
            CoverageSegment::new(5, 7, CoverageKind::Data).unwrap(),
        ];

        assert_eq!(
            missing_segments_for_request(1, 10, &segments),
            vec![ClosedInterval::new(8, 10).unwrap()],
        );
    }

    #[rstest]
    fn coverage_intervals_by_kind_keeps_data_out_of_empty_ranges() {
        let segments = [
            CoverageSegment::new(1, 10, CoverageKind::Empty).unwrap(),
            CoverageSegment::new(4, 6, CoverageKind::Data).unwrap(),
        ];

        assert_eq!(
            coverage_intervals_by_kind(&segments),
            CoverageIntervals {
                data: vec![ClosedInterval::new(4, 6).unwrap()],
                empty: vec![
                    ClosedInterval::new(1, 3).unwrap(),
                    ClosedInterval::new(7, 10).unwrap(),
                ],
            },
        );
    }

    #[rstest]
    fn missing_segments_for_request_applies_delete_tombstones_in_order() {
        let segments = [
            CoverageSegment::new(10, 30, CoverageKind::Data).unwrap(),
            CoverageSegment::new(15, 25, CoverageKind::Deleted).unwrap(),
            CoverageSegment::new(20, 22, CoverageKind::Data).unwrap(),
        ];

        assert_eq!(
            missing_segments_for_request(10, 30, &segments),
            vec![
                ClosedInterval::new(15, 19).unwrap(),
                ClosedInterval::new(23, 25).unwrap(),
            ],
        );
    }

    #[rstest]
    fn missing_intervals_handles_overlaps_and_invalid_requests() {
        let intervals = [
            ClosedInterval::new(10, 30).unwrap(),
            ClosedInterval::new(20, 40).unwrap(),
            ClosedInterval::new(60, 80).unwrap(),
        ];

        assert_eq!(
            missing_intervals(1, 100, &intervals),
            vec![
                ClosedInterval::new(1, 9).unwrap(),
                ClosedInterval::new(41, 59).unwrap(),
                ClosedInterval::new(81, 100).unwrap(),
            ],
        );
        assert_eq!(missing_intervals(100, 1, &intervals), Vec::new());
    }
}
