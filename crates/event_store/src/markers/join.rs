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

//! Catalog joins for folded data marker cursors.

use std::fmt::Display;

use ahash::AHashMap;
use nautilus_core::UnixNanos;

use crate::{
    error::EventStoreError,
    markers::{DataClass, MarkerReader, StreamCursor, StreamDictEntry, StreamSlot},
    replay::{
        CatalogReplayRecord, CatalogSliceCoverage, CatalogSlicePlan, CatalogSliceQuery,
        ReplayCatalog,
    },
};

/// Catalog rows joined to one folded marker stream cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedStream {
    /// Stream dictionary entry for the folded cursor.
    pub entry: StreamDictEntry,
    /// Folded cursor for the requested event-store boundary.
    pub cursor: StreamCursor,
    /// Catalog rows selected for replayable data classes.
    pub records: Vec<CatalogReplayRecord>,
    /// Whether the catalog row count disagrees with the folded marker count.
    pub candidate: bool,
}

/// Joins folded marker cursors at `event_seq_before` to replayable catalog rows.
///
/// Quote, trade, and bar streams plan and load catalog slices from the beginning of the
/// catalog through the folded cursor's `ts_init_hi`, then return the first `count` rows.
/// Order-book streams remain order-only and return no catalog rows.
///
/// # Errors
///
/// Returns [`EventStoreError`] when marker cursor folding fails, when a folded slot has no
/// dictionary entry, when `count` does not fit in memory on this platform, or when the catalog
/// fails while planning or loading a replayable slice.
pub fn join_at_entry<C>(
    reader: &MarkerReader,
    catalog: &mut C,
    event_seq_before: u64,
) -> Result<Vec<JoinedStream>, EventStoreError>
where
    C: ReplayCatalog + ?Sized,
{
    let mut folded: Vec<_> = reader.fold_to(event_seq_before)?.into_values().collect();
    folded.sort_by_key(|cursor| cursor.slot);
    let dict = reader.stream_dictionary()?;

    folded
        .into_iter()
        .map(|cursor| join_stream(&dict, catalog, cursor))
        .collect()
}

fn join_stream<C>(
    dict: &AHashMap<StreamSlot, StreamDictEntry>,
    catalog: &mut C,
    cursor: StreamCursor,
) -> Result<JoinedStream, EventStoreError>
where
    C: ReplayCatalog + ?Sized,
{
    let entry = dict.get(&cursor.slot).cloned().ok_or_else(|| {
        EventStoreError::Backend(format!(
            "marker stream slot {} missing from dictionary",
            cursor.slot
        ))
    })?;

    let Some(data_cls) = replayable_data_class(entry.data_cls) else {
        return Ok(JoinedStream {
            entry,
            cursor,
            records: Vec::new(),
            candidate: false,
        });
    };

    let expected_count = usize::try_from(cursor.count).map_err(|_| {
        EventStoreError::Backend(format!(
            "marker cursor count {} does not fit in memory",
            cursor.count
        ))
    })?;
    let mut records = load_replayable_records(catalog, &entry, &cursor, data_cls)?;
    let candidate = records.len() != expected_count;
    records.truncate(expected_count);

    Ok(JoinedStream {
        entry,
        cursor,
        records,
        candidate,
    })
}

fn load_replayable_records<C>(
    catalog: &mut C,
    entry: &StreamDictEntry,
    cursor: &StreamCursor,
    data_cls: &str,
) -> Result<Vec<CatalogReplayRecord>, EventStoreError>
where
    C: ReplayCatalog + ?Sized,
{
    let query = CatalogSliceQuery {
        data_cls: data_cls.to_string(),
        identifiers: vec![entry.identifier.clone()],
        start: UnixNanos::from(0),
        end: cursor.ts_init_hi,
        required: false,
    };
    let coverage = plan_slice(catalog, &query)?;
    let plan = CatalogSlicePlan { query, coverage };

    if plan.is_missing() {
        Ok(Vec::new())
    } else {
        load_slice(catalog, &plan)
    }
}

fn replayable_data_class(data_cls: DataClass) -> Option<&'static str> {
    match data_cls {
        DataClass::Quote => Some("quotes"),
        DataClass::Trade => Some("trades"),
        DataClass::Bar => Some("bars"),
        DataClass::BookDeltas | DataClass::BookDepth10 => None,
    }
}

fn plan_slice<C>(
    catalog: &mut C,
    query: &CatalogSliceQuery,
) -> Result<CatalogSliceCoverage, EventStoreError>
where
    C: ReplayCatalog + ?Sized,
{
    catalog
        .plan_slice(query)
        .map_err(|e| catalog_error(&query.data_cls, "plan", e))
}

fn load_slice<C>(
    catalog: &mut C,
    plan: &CatalogSlicePlan,
) -> Result<Vec<CatalogReplayRecord>, EventStoreError>
where
    C: ReplayCatalog + ?Sized,
{
    catalog
        .load_slice(plan)
        .map_err(|e| catalog_error(&plan.query.data_cls, "load", e))
}

fn catalog_error(data_cls: &str, action: &str, error: impl Display) -> EventStoreError {
    EventStoreError::Backend(format!(
        "catalog marker join {action} failed for {data_cls}: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Bar, BarType, QuoteTick, TradeTick},
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        manifest::RunStatus,
        markers::{
            DataClass, DataCursorSnapshot, HiFiMarker, MarkerBackend, MarkerGap, MarkerManifest,
            MarkerReader, MemoryMarkerBackend, StreamCursor, StreamDictEntry, compute_dict_hash,
            compute_marker_hash,
        },
        replay::{
            CatalogReplayData, CatalogReplayRecord, CatalogSliceCoverage, CatalogSlicePlan,
            CatalogSliceQuery, ReplayCatalog,
        },
    };

    #[rstest]
    fn join_resolves_replayable_catalog_slices_for_entry() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let trade = dict(1, DataClass::Trade, "AUD/USD.SIM");
        let bar_type = "AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL";
        let bar = dict(2, DataClass::Bar, bar_type);
        let reader = reader_with(
            vec![quote.clone(), trade.clone(), bar.clone()],
            vec![
                snapshot(
                    1,
                    5,
                    vec![
                        cursor(0, 2_000, 2),
                        cursor(1, 3_000, 1),
                        cursor(2, 4_000, 2),
                    ],
                ),
                snapshot(2, 11, vec![cursor(0, 3_000, 3)]),
            ],
        );
        let mut catalog = StubReplayCatalog::new(vec![
            quote_record(1_000),
            quote_record(2_000),
            quote_record(3_000),
            trade_record(3_000),
            bar_record(bar_type, 1_000),
            bar_record(bar_type, 4_000),
            bar_record(bar_type, 5_000),
        ]);

        let joined = join_at_entry(&reader, &mut catalog, 5).expect("join");

        assert_eq!(joined.len(), 3);
        assert_join(
            &joined[0],
            quote,
            cursor(0, 2_000, 2),
            &["quotes", "quotes"],
            false,
        );
        assert_join(&joined[1], trade, cursor(1, 3_000, 1), &["trades"], false);
        assert_join(
            &joined[2],
            bar,
            cursor(2, 4_000, 2),
            &["bars", "bars"],
            false,
        );
        assert_eq!(
            catalog.plan_queries,
            vec![
                query("quotes", "AUD/USD.SIM", 2_000),
                query("trades", "AUD/USD.SIM", 3_000),
                query("bars", bar_type, 4_000),
            ],
        );
    }

    #[rstest]
    fn count_mismatch_flags_candidate() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let reader = reader_with(
            vec![quote.clone()],
            vec![snapshot(1, 5, vec![cursor(0, 2_000, 3)])],
        );
        let mut catalog = StubReplayCatalog::new(vec![quote_record(1_000), quote_record(2_000)]);

        let joined = join_at_entry(&reader, &mut catalog, 5).expect("join");

        assert_eq!(joined.len(), 1);
        assert_join(
            &joined[0],
            quote,
            cursor(0, 2_000, 3),
            &["quotes", "quotes"],
            true,
        );
    }

    #[rstest]
    fn over_count_mismatch_truncates_to_marker_count() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let reader = reader_with(
            vec![quote.clone()],
            vec![snapshot(1, 5, vec![cursor(0, 2_000, 2)])],
        );
        let mut catalog = StubReplayCatalog::new(vec![
            quote_record(1_000),
            quote_record(1_500),
            quote_record(2_000),
        ]);

        let joined = join_at_entry(&reader, &mut catalog, 5).expect("join");

        assert_eq!(joined.len(), 1);
        assert_join(
            &joined[0],
            quote,
            cursor(0, 2_000, 2),
            &["quotes", "quotes"],
            true,
        );
        assert_eq!(
            joined[0]
                .records
                .iter()
                .map(|record| record.ts_init)
                .collect::<Vec<_>>(),
            vec![UnixNanos::from(1_000), UnixNanos::from(1_500)],
        );
    }

    #[rstest]
    #[case::deltas(DataClass::BookDeltas)]
    #[case::depth10(DataClass::BookDepth10)]
    fn order_book_streams_resolve_order_only(#[case] data_cls: DataClass) {
        let order_book = dict(0, data_cls, "AUD/USD.SIM");
        let reader = reader_with(
            vec![order_book.clone()],
            vec![snapshot(1, 5, vec![cursor(0, 2_000, 3)])],
        );
        let mut catalog = StubReplayCatalog::new(vec![quote_record(1_000)]);

        let joined = join_at_entry(&reader, &mut catalog, 5).expect("join");

        assert_eq!(joined.len(), 1);
        assert_join(&joined[0], order_book, cursor(0, 2_000, 3), &[], false);
        assert!(catalog.plan_queries.is_empty());
        assert!(catalog.load_plans.is_empty());
    }

    #[rstest]
    fn missing_optional_catalog_slice_flags_candidate_without_loading() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let reader = reader_with(
            vec![quote.clone()],
            vec![snapshot(1, 5, vec![cursor(0, 2_000, 2)])],
        );
        let mut catalog = StubReplayCatalog::new(Vec::new()).with_coverage_files(Vec::new());

        let joined = join_at_entry(&reader, &mut catalog, 5).expect("join");

        assert_eq!(joined.len(), 1);
        assert_join(&joined[0], quote, cursor(0, 2_000, 2), &[], true);
        assert_eq!(
            catalog.plan_queries,
            vec![query("quotes", "AUD/USD.SIM", 2_000)],
        );
        assert!(catalog.load_plans.is_empty());
    }

    #[rstest]
    fn missing_stream_dict_entry_returns_error() {
        let reader = reader_with(Vec::new(), vec![snapshot(1, 5, vec![cursor(0, 2_000, 1)])]);
        let mut catalog = StubReplayCatalog::new(Vec::new());

        let err = join_at_entry(&reader, &mut catalog, 5).expect_err("missing dict must fail");

        match err {
            EventStoreError::Backend(message) => {
                assert!(
                    message.contains("marker stream slot 0 missing from dictionary"),
                    "message was: {message}",
                );
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn dictionary_scan_error_is_preserved() {
        let reader = MarkerReader::new(Box::new(FailingDictBackend));
        let mut catalog = StubReplayCatalog::new(Vec::new());

        let err = join_at_entry(&reader, &mut catalog, 5).expect_err("dict scan must fail");

        match err {
            EventStoreError::Backend(message) => {
                assert_eq!(message, "scan dict failed");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn catalog_plan_error_is_wrapped() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let reader = reader_with(vec![quote], vec![snapshot(1, 5, vec![cursor(0, 2_000, 1)])]);
        let mut catalog = StubReplayCatalog::new(Vec::new()).with_plan_error("offline");

        let err = join_at_entry(&reader, &mut catalog, 5).expect_err("plan error must fail");

        match err {
            EventStoreError::Backend(message) => {
                assert_eq!(
                    message,
                    "catalog marker join plan failed for quotes: offline",
                );
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn catalog_load_error_is_wrapped() {
        let quote = dict(0, DataClass::Quote, "AUD/USD.SIM");
        let reader = reader_with(vec![quote], vec![snapshot(1, 5, vec![cursor(0, 2_000, 1)])]);
        let mut catalog = StubReplayCatalog::new(Vec::new()).with_load_error("decode failed");

        let err = join_at_entry(&reader, &mut catalog, 5).expect_err("load error must fail");

        match err {
            EventStoreError::Backend(message) => {
                assert_eq!(
                    message,
                    "catalog marker join load failed for quotes: decode failed",
                );
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    struct StubReplayCatalog {
        records: Vec<CatalogReplayRecord>,
        plan_queries: Vec<CatalogSliceQuery>,
        load_plans: Vec<CatalogSlicePlan>,
        coverage_files: Option<Vec<String>>,
        plan_error: Option<String>,
        load_error: Option<String>,
    }

    impl StubReplayCatalog {
        fn new(records: Vec<CatalogReplayRecord>) -> Self {
            Self {
                records,
                plan_queries: Vec::new(),
                load_plans: Vec::new(),
                coverage_files: None,
                plan_error: None,
                load_error: None,
            }
        }

        fn with_coverage_files(mut self, files: Vec<String>) -> Self {
            self.coverage_files = Some(files);
            self
        }

        fn with_plan_error(mut self, message: &str) -> Self {
            self.plan_error = Some(message.to_string());
            self
        }

        fn with_load_error(mut self, message: &str) -> Self {
            self.load_error = Some(message.to_string());
            self
        }
    }

    impl ReplayCatalog for StubReplayCatalog {
        type Error = String;

        fn plan_slice(
            &mut self,
            query: &CatalogSliceQuery,
        ) -> Result<CatalogSliceCoverage, Self::Error> {
            if let Some(error) = &self.plan_error {
                return Err(error.clone());
            }

            self.plan_queries.push(query.clone());
            Ok(CatalogSliceCoverage::from_files(
                self.coverage_files.clone().unwrap_or_else(|| {
                    vec![format!("{}/{}", query.data_cls, query.identifiers[0])]
                }),
            ))
        }

        fn load_slice(
            &mut self,
            plan: &CatalogSlicePlan,
        ) -> Result<Vec<CatalogReplayRecord>, Self::Error> {
            if let Some(error) = &self.load_error {
                return Err(error.clone());
            }

            self.load_plans.push(plan.clone());
            Ok(self
                .records
                .iter()
                .filter(|record| record_matches_query(record, &plan.query))
                .cloned()
                .collect())
        }
    }

    fn reader_with(
        dict_entries: Vec<StreamDictEntry>,
        snapshots: Vec<DataCursorSnapshot>,
    ) -> MarkerReader {
        let mut backend = MemoryMarkerBackend::new();
        backend.open_run(manifest()).expect("open run");
        for entry in dict_entries {
            backend
                .put_dict(&entry, compute_dict_hash(&entry))
                .expect("put dict");
        }

        for snapshot in snapshots {
            backend
                .append_snapshot(&snapshot, compute_marker_hash(&snapshot))
                .expect("append snapshot");
        }
        MarkerReader::new(Box::new(backend))
    }

    fn manifest() -> MarkerManifest {
        MarkerManifest {
            run_id: "1700000000-join".to_string(),
            enabled_classes: vec![DataClass::Quote, DataClass::Trade, DataClass::Bar],
            high_fidelity: false,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn snapshot(
        marker_seq: u64,
        event_seq_before: u64,
        advanced: Vec<StreamCursor>,
    ) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced,
        }
    }

    fn cursor(slot: u32, ts_init_hi: u64, count: u64) -> StreamCursor {
        StreamCursor {
            slot,
            ts_init_hi: UnixNanos::from(ts_init_hi),
            count,
        }
    }

    fn dict(slot: u32, data_cls: DataClass, identifier: &str) -> StreamDictEntry {
        StreamDictEntry {
            slot,
            data_cls,
            identifier: identifier.to_string(),
        }
    }

    fn quote_record(ts_init: u64) -> CatalogReplayRecord {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        CatalogReplayRecord::from_data(CatalogReplayData::Quote(QuoteTick::new(
            instrument_id,
            Price::from("1.0001"),
            Price::from("1.0002"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )))
    }

    fn trade_record(ts_init: u64) -> CatalogReplayRecord {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        CatalogReplayRecord::from_data(CatalogReplayData::Trade(TradeTick::new(
            instrument_id,
            Price::from("1.0001"),
            Quantity::from("100"),
            AggressorSide::Buyer,
            TradeId::from("T-1"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )))
    }

    fn bar_record(bar_type: &str, ts_init: u64) -> CatalogReplayRecord {
        CatalogReplayRecord::from_data(CatalogReplayData::Bar(Bar::new(
            BarType::from(bar_type),
            Price::from("1.0000"),
            Price::from("1.0002"),
            Price::from("1.0000"),
            Price::from("1.0001"),
            Quantity::from("100"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )))
    }

    fn query(data_cls: &str, identifier: &str, end: u64) -> CatalogSliceQuery {
        CatalogSliceQuery {
            data_cls: data_cls.to_string(),
            identifiers: vec![identifier.to_string()],
            start: UnixNanos::from(0),
            end: UnixNanos::from(end),
            required: false,
        }
    }

    fn record_matches_query(record: &CatalogReplayRecord, query: &CatalogSliceQuery) -> bool {
        record.data_cls == query.data_cls
            && record
                .identifier
                .as_ref()
                .is_some_and(|identifier| query.identifiers.contains(identifier))
            && record.ts_init >= query.start
            && record.ts_init <= query.end
    }

    fn assert_join(
        joined: &JoinedStream,
        entry: StreamDictEntry,
        cursor: StreamCursor,
        data_classes: &[&str],
        candidate: bool,
    ) {
        assert_eq!(joined.entry, entry);
        assert_eq!(joined.cursor, cursor);
        assert_eq!(
            joined
                .records
                .iter()
                .map(|record| record.data_cls.as_str())
                .collect::<Vec<_>>(),
            data_classes,
        );
        assert_eq!(joined.candidate, candidate);
    }

    #[derive(Debug)]
    struct FailingDictBackend;

    impl MarkerBackend for FailingDictBackend {
        fn open_run(&mut self, _: MarkerManifest) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn append_snapshot(
            &mut self,
            _: &DataCursorSnapshot,
            _: [u8; 32],
        ) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn append_hifi(&mut self, _: &HiFiMarker, _: [u8; 32]) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn append_gap(&mut self, _: &MarkerGap, _: [u8; 32]) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn put_dict(&mut self, _: &StreamDictEntry, _: [u8; 32]) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
            Ok(vec![snapshot(1, 5, vec![cursor(0, 2_000, 1)])])
        }

        fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
            unreachable!("test does not scan high-fidelity markers")
        }

        fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
            unreachable!("test does not scan gaps")
        }

        fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
            Err(EventStoreError::Backend("scan dict failed".to_string()))
        }

        fn seal(&mut self, _: RunStatus) -> Result<(), EventStoreError> {
            unreachable!("test backend is read-only")
        }

        fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
            unreachable!("test does not read manifest")
        }
    }
}
