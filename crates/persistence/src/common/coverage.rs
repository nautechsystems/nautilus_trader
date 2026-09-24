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

    fn closed_intervals(pairs: &[(u64, u64)]) -> Vec<ClosedInterval> {
        pairs
            .iter()
            .map(|&(start, end)| ClosedInterval::new(start, end).unwrap())
            .collect()
    }

    fn coverage_row(
        start_ts: u64,
        end_ts: u64,
        kind: CoverageKind,
        data_version: Option<i64>,
        created_ts: u64,
    ) -> CatalogCoverageRow {
        CatalogCoverageRow {
            table_path: "quotes".to_string(),
            data_type: "QuoteTick".to_string(),
            identifier: Some("AUD/USD.SIM".to_string()),
            start_ts,
            end_ts,
            kind,
            row_count: 5,
            data_version,
            source: "writer".to_string(),
            created_ts,
            schema_version: COVERAGE_SCHEMA_VERSION,
        }
    }

    #[rstest]
    #[case::single_nanosecond_request(5, 5, &[], &[(5, 5)])]
    #[case::interval_ends_at_request_start(5, 10, &[(1, 5)], &[(6, 10)])]
    #[case::interval_starts_at_request_end(1, 10, &[(10, 15)], &[(1, 9)])]
    #[case::interval_after_request(1, 5, &[(10, 15)], &[(1, 5)])]
    #[case::interval_before_request(10, 20, &[(1, 5), (8, 12)], &[(13, 20)])]
    fn missing_intervals_handles_request_boundaries(
        #[case] request_start: u64,
        #[case] request_end: u64,
        #[case] intervals: &[(u64, u64)],
        #[case] expected: &[(u64, u64)],
    ) {
        let missing = missing_intervals(request_start, request_end, &closed_intervals(intervals));

        assert_eq!(missing, closed_intervals(expected));
    }

    #[rstest]
    #[case::deletes_first_nanosecond(&[(10, 20)], (5, 10), &[(11, 20)])]
    #[case::deletes_last_nanosecond(&[(10, 20)], (20, 25), &[(10, 19)])]
    #[case::keeps_interval_after_deletion(&[(10, 20)], (1, 5), &[(10, 20)])]
    #[case::keeps_interval_before_deletion(&[(10, 20)], (25, 30), &[(10, 20)])]
    #[case::deletes_shared_start(&[(10, 20)], (10, 15), &[(16, 20)])]
    #[case::deletes_shared_end(&[(10, 20)], (15, 20), &[(10, 14)])]
    #[case::merges_before_deleting(&[(16, 20), (10, 15)], (12, 18), &[(10, 11), (19, 20)])]
    fn subtract_interval_from_intervals_handles_boundaries(
        #[case] intervals: &[(u64, u64)],
        #[case] deleted: (u64, u64),
        #[case] expected: &[(u64, u64)],
    ) {
        let deleted = ClosedInterval::new(deleted.0, deleted.1).unwrap();

        let remaining = subtract_interval_from_intervals(&closed_intervals(intervals), deleted);

        assert_eq!(remaining, closed_intervals(expected));
    }

    #[rstest]
    fn coverage_intervals_by_kind_applies_deletes_to_data_and_empty() {
        let segments = [
            CoverageSegment::new(1, 10, CoverageKind::Data).unwrap(),
            CoverageSegment::new(11, 20, CoverageKind::Empty).unwrap(),
            CoverageSegment::new(5, 15, CoverageKind::Deleted).unwrap(),
        ];

        assert_eq!(
            coverage_intervals_by_kind(&segments),
            CoverageIntervals {
                data: closed_intervals(&[(1, 4)]),
                empty: closed_intervals(&[(16, 20)]),
            },
        );
    }

    #[rstest]
    #[case::deleted_tail(
        &[(10, 20, CoverageKind::Data), (21, 40, CoverageKind::Empty), (15, 20, CoverageKind::Deleted)],
        Some(14),
    )]
    #[case::data_after_delete(
        &[(10, 20, CoverageKind::Data), (5, 30, CoverageKind::Deleted), (12, 13, CoverageKind::Data)],
        Some(13),
    )]
    #[case::inverted_row_skipped(
        &[(10, 20, CoverageKind::Data), (30, 25, CoverageKind::Data)],
        Some(20),
    )]
    #[case::empty_only(&[(1, 5, CoverageKind::Empty)], None)]
    fn last_data_timestamp_applies_rows_in_order(
        #[case] rows: &[(u64, u64, CoverageKind)],
        #[case] expected: Option<u64>,
    ) {
        let rows = rows
            .iter()
            .map(|&(start_ts, end_ts, kind)| coverage_row(start_ts, end_ts, kind, None, 1))
            .collect::<Vec<_>>();

        assert_eq!(last_data_timestamp(&rows), expected);
    }

    #[rstest]
    fn deduplicate_and_sort_coverage_rows_keeps_causal_latest_per_kind() {
        let older_data = coverage_row(10, 20, CoverageKind::Data, Some(1), 100);
        let newer_data = coverage_row(10, 20, CoverageKind::Data, Some(2), 50);
        let deleted = coverage_row(10, 20, CoverageKind::Deleted, Some(1), 200);
        let empty = coverage_row(30, 40, CoverageKind::Empty, Some(1), 150);
        let unversioned = coverage_row(1, 5, CoverageKind::Data, None, 300);

        let rows = deduplicate_and_sort_coverage_rows(vec![
            older_data,
            deleted.clone(),
            newer_data.clone(),
            empty.clone(),
            unversioned.clone(),
        ]);

        assert_eq!(rows, vec![unversioned, empty, deleted, newer_data]);
    }

    #[rstest]
    fn coverage_rows_round_trip_through_record_batch() {
        let rows = vec![
            CatalogCoverageRow {
                table_path: "quotes".to_string(),
                data_type: "QuoteTick".to_string(),
                identifier: None,
                start_ts: 1,
                end_ts: 5,
                kind: CoverageKind::Data,
                row_count: 3,
                data_version: None,
                source: "writer".to_string(),
                created_ts: 100,
                schema_version: COVERAGE_SCHEMA_VERSION,
            },
            CatalogCoverageRow {
                table_path: "bars".to_string(),
                data_type: "Bar".to_string(),
                identifier: Some("ES.GLBX".to_string()),
                start_ts: 6,
                end_ts: 9,
                kind: CoverageKind::Deleted,
                row_count: 0,
                data_version: Some(7),
                source: "delete".to_string(),
                created_ts: 200,
                schema_version: 2,
            },
            CatalogCoverageRow {
                table_path: "trades".to_string(),
                data_type: "TradeTick".to_string(),
                identifier: Some("ETHUSDT.BINANCE".to_string()),
                start_ts: 10,
                end_ts: 12,
                kind: CoverageKind::Empty,
                row_count: 0,
                data_version: Some(0),
                source: "request".to_string(),
                created_ts: 300,
                schema_version: 3,
            },
        ];

        let batch = coverage_rows_to_batch(&rows).unwrap();

        assert_eq!(decode_coverage_batches(vec![batch]).unwrap(), rows);
    }

    #[rstest]
    fn decode_coverage_batches_rejects_unknown_status() {
        let batch =
            coverage_rows_to_batch(&[coverage_row(1, 5, CoverageKind::Data, None, 1)]).unwrap();
        let status_index = batch.schema().index_of(STATUS_COLUMN).unwrap();
        let mut columns = batch.columns().to_vec();
        columns[status_index] = Arc::new(StringArray::from(vec!["partial"]));
        let batch = RecordBatch::try_new(batch.schema(), columns).unwrap();

        let error = decode_coverage_batches(vec![batch]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown catalog coverage status: partial"
        );
    }

    #[rstest]
    fn rows_to_segments_skips_inverted_rows() {
        let rows = [
            coverage_row(1, 5, CoverageKind::Data, None, 1),
            coverage_row(9, 3, CoverageKind::Deleted, None, 2),
        ];

        assert_eq!(
            rows_to_segments(&rows),
            vec![CoverageSegment::new(1, 5, CoverageKind::Data).unwrap()],
        );
    }

    fn ts_init_batch() -> RecordBatch {
        RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("ts_init", DataType::UInt64, false),
                Field::new("value", DataType::UInt32, false),
            ])),
            vec![
                Arc::new(UInt64Array::from(vec![1_u64, 5, 10, 15])),
                Arc::new(UInt32Array::from(vec![1_u32, 2, 3, 4])),
            ],
        )
        .unwrap()
    }

    #[rstest]
    #[case::selects_rows_inside_bounds(&[(5, 10)], Some((vec![2, 3], vec![5, 10])))]
    #[case::selects_every_row(&[(0, 20)], Some((vec![1, 2, 3, 4], vec![1, 5, 10, 15])))]
    #[case::selects_across_intervals(&[(0, 1), (15, 15)], Some((vec![1, 4], vec![1, 15])))]
    #[case::selects_no_row(&[(20, 30)], None)]
    fn filter_record_batch_by_ts_init_intervals_selects_rows(
        #[case] intervals: &[(u64, u64)],
        #[case] expected: Option<(Vec<u32>, Vec<u64>)>,
    ) {
        let filtered = filter_record_batch_by_ts_init_intervals(
            &ts_init_batch(),
            &closed_intervals(intervals),
        )
        .unwrap()
        .map(|(batch, timestamps)| {
            let values = batch
                .column_by_name("value")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt32Array>()
                .unwrap()
                .values()
                .to_vec();
            (values, timestamps)
        });

        assert_eq!(filtered, expected);
    }

    #[rstest]
    fn filter_record_batch_by_ts_init_intervals_requires_ts_init() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "value",
                DataType::UInt32,
                false,
            )])),
            vec![Arc::new(UInt32Array::from(vec![1_u32]))],
        )
        .unwrap();

        let error = filter_record_batch_by_ts_init_intervals(&batch, &closed_intervals(&[(0, 1)]))
            .unwrap_err();

        assert_eq!(error.to_string(), "ts_init column not found");
    }
}
