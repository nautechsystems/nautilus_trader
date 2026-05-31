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

//! Durable schema types and canonical content hashes for the data marker sidecar.

use std::fmt::Display;

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::wire;

const MARKER_HASH_DOMAIN: &[u8] = b"nautilus-event-store/marker/v1";
const HIFI_HASH_DOMAIN: &[u8] = b"nautilus-event-store/hifi/v1";

/// The class of market-data stream being tracked by a sidecar slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataClass {
    /// Order-book delta stream.
    BookDeltas,
    /// Level-10 order-book snapshot stream.
    BookDepth10,
    /// Quote (level-1 bid/ask) stream.
    Quote,
    /// Trade (last sale) stream.
    Trade,
    /// Bar (OHLCV aggregate) stream.
    Bar,
}

impl DataClass {
    /// Returns the canonical string representation of this data class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BookDeltas => "BookDeltas",
            Self::BookDepth10 => "BookDepth10",
            Self::Quote => "Quote",
            Self::Trade => "Trade",
            Self::Bar => "Bar",
        }
    }
}

impl Display for DataClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DataClass {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "BookDeltas" => Ok(Self::BookDeltas),
            "BookDepth10" => Ok(Self::BookDepth10),
            "Quote" => Ok(Self::Quote),
            "Trade" => Ok(Self::Trade),
            "Bar" => Ok(Self::Bar),
            other => Err(format!("unknown DataClass, was `{other}`")),
        }
    }
}

/// A slot index identifying a registered market-data stream.
pub type StreamSlot = u32;

/// The cursor position within a single market-data stream slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCursor {
    /// The stream slot index.
    pub slot: StreamSlot,
    /// The highest `ts_init` observed so far in this slot.
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_init_hi: UnixNanos,
    /// The number of records observed so far in this slot.
    pub count: u64,
}

/// A snapshot of the cursor positions for all active market-data streams at a marker point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataCursorSnapshot {
    /// Monotonic sequence of this marker within the sidecar.
    pub marker_seq: u64,
    /// The event-store sequence before which this snapshot was taken.
    pub event_seq_before: u64,
    /// The `ts_init` at the point this snapshot was taken.
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_init: UnixNanos,
    /// The cursors for every stream slot that advanced since the previous snapshot.
    pub advanced: Vec<StreamCursor>,
}

/// A high-fidelity per-record marker capturing per-record identity within a stream slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiFiMarker {
    /// Monotonic sequence of this marker within the sidecar.
    pub marker_seq: u64,
    /// The event-store sequence before which this marker was recorded.
    pub event_seq_before: u64,
    /// The stream slot index for this record.
    pub slot: StreamSlot,
    /// The domain timestamp of the record (`ts_event`).
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_event: UnixNanos,
    /// The ingestion timestamp of the record (`ts_init`).
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_init: UnixNanos,
    /// Ordinal among records sharing the same `ts_init` within a slot.
    pub same_ts_ordinal: u32,
    /// A 32-byte fingerprint of the record's content.
    pub record_fingerprint: [u8; 32],
}

/// The reason a gap exists in the sidecar marker sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerGapReason {
    /// The marker ring-buffer overflowed; some markers were dropped.
    Overflow,
    /// The marker writer was closed before flushing.
    WriterClosed,
}

/// A gap in the sidecar marker sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerGap {
    /// The first marker sequence number missing from the sequence.
    pub from_marker_seq: u64,
    /// The first marker sequence number after the gap.
    pub to_marker_seq: u64,
    /// The reason this gap was recorded.
    pub reason: MarkerGapReason,
}

/// A registry entry mapping a stream slot to its data class and instrument identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamDictEntry {
    /// The stream slot index.
    pub slot: StreamSlot,
    /// The data class of this stream.
    pub data_cls: DataClass,
    /// The instrument identifier string for this stream.
    pub identifier: String,
}

/// Computes the canonical BLAKE3 hash of a [`DataCursorSnapshot`].
///
/// The hash is domain-separated by a crate-internal prefix, writes numeric fields big-endian
/// in declared order, and length-prefixes the variable-length cursor list so two distinct
/// snapshots cannot frame to the same byte stream. Store the returned bytes alongside the
/// record; do not add a hash field to the struct itself.
#[must_use]
pub fn compute_marker_hash(snapshot: &DataCursorSnapshot) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(MARKER_HASH_DOMAIN);
    hasher.update(&snapshot.marker_seq.to_be_bytes());
    hasher.update(&snapshot.event_seq_before.to_be_bytes());
    hasher.update(&snapshot.ts_init.as_u64().to_be_bytes());
    hasher.update(&(snapshot.advanced.len() as u64).to_be_bytes());
    for cursor in &snapshot.advanced {
        hasher.update(&cursor.slot.to_be_bytes());
        hasher.update(&cursor.ts_init_hi.as_u64().to_be_bytes());
        hasher.update(&cursor.count.to_be_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Computes the canonical BLAKE3 hash of a [`HiFiMarker`].
///
/// The hash is domain-separated by a crate-internal prefix and uses big-endian fixed-width
/// framing for every field. Store the returned bytes alongside the record; do not add a hash
/// field to the struct itself.
#[must_use]
pub fn compute_hifi_hash(marker: &HiFiMarker) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(HIFI_HASH_DOMAIN);
    hasher.update(&marker.marker_seq.to_be_bytes());
    hasher.update(&marker.event_seq_before.to_be_bytes());
    hasher.update(&marker.slot.to_be_bytes());
    hasher.update(&marker.ts_event.as_u64().to_be_bytes());
    hasher.update(&marker.ts_init.as_u64().to_be_bytes());
    hasher.update(&marker.same_ts_ordinal.to_be_bytes());
    hasher.update(&marker.record_fingerprint);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write, str::FromStr};

    use proptest::{prelude::*, test_runner::Config as ProptestConfig};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn data_class_roundtrips_to_str() {
        let variants = [
            (DataClass::BookDeltas, "BookDeltas"),
            (DataClass::BookDepth10, "BookDepth10"),
            (DataClass::Quote, "Quote"),
            (DataClass::Trade, "Trade"),
            (DataClass::Bar, "Bar"),
        ];

        for (variant, expected) in variants {
            assert_eq!(variant.as_str(), expected, "as_str for {variant:?}");
            assert_eq!(variant.to_string(), expected, "Display for {variant:?}");
            assert_eq!(
                DataClass::from_str(expected).unwrap(),
                variant,
                "from_str for {expected}"
            );
        }
    }

    fn baseline_snapshot() -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq: 1,
            event_seq_before: 42,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000),
            advanced: vec![
                StreamCursor {
                    slot: 0,
                    ts_init_hi: UnixNanos::from(1_700_000_000_000_000_001),
                    count: 7,
                },
                StreamCursor {
                    slot: 1,
                    ts_init_hi: UnixNanos::from(1_700_000_000_000_000_002),
                    count: 3,
                },
            ],
        }
    }

    fn baseline_hifi() -> HiFiMarker {
        HiFiMarker {
            marker_seq: 1,
            event_seq_before: 42,
            slot: 0,
            ts_event: UnixNanos::from(1_700_000_000_000_000_000),
            ts_init: UnixNanos::from(1_700_000_000_000_000_001),
            same_ts_ordinal: 0,
            record_fingerprint: [0xABu8; 32],
        }
    }

    fn hex32(bytes: &[u8; 32]) -> String {
        let mut out = String::with_capacity(64);
        for byte in bytes {
            write!(out, "{byte:02x}").expect("writing to a String is infallible");
        }
        out
    }

    #[rstest]
    fn marker_hash_is_deterministic() {
        let snap = baseline_snapshot();
        let h1 = compute_marker_hash(&snap);
        let h2 = compute_marker_hash(&snap);

        assert_eq!(h1, h2);

        // Pinned wire-format vector. Any change to domain, field order, or endianness flips
        // this value.
        let hex = hex32(&h1);
        assert_eq!(
            hex, "898bc3efdaf0edd9167a38a1c3060c9b4dc051658ea2f6132004bed78a481c47",
            "marker hash wire format changed"
        );
    }

    #[rstest]
    fn hifi_hash_is_deterministic() {
        let marker = baseline_hifi();
        let h1 = compute_hifi_hash(&marker);
        let h2 = compute_hifi_hash(&marker);

        assert_eq!(h1, h2);

        let hex = hex32(&h1);
        assert_eq!(
            hex, "06542408380d8815ef783b9dbde6b3e3ffdf05605bb17e83ad48474557457517",
            "hifi hash wire format changed"
        );
    }

    #[rstest]
    fn marker_record_bincode_roundtrip() {
        let cfg = bincode::config::standard();

        // DataCursorSnapshot
        let snap = baseline_snapshot();
        let bytes = bincode::serde::encode_to_vec(&snap, cfg).unwrap();
        let (decoded, _): (DataCursorSnapshot, _) =
            bincode::serde::decode_from_slice(&bytes, cfg).unwrap();
        assert_eq!(snap, decoded);

        // HiFiMarker
        let hifi = baseline_hifi();
        let bytes = bincode::serde::encode_to_vec(&hifi, cfg).unwrap();
        let (decoded, _): (HiFiMarker, _) = bincode::serde::decode_from_slice(&bytes, cfg).unwrap();
        assert_eq!(hifi, decoded);

        // MarkerGap
        let gap = MarkerGap {
            from_marker_seq: 5,
            to_marker_seq: 10,
            reason: MarkerGapReason::Overflow,
        };
        let bytes = bincode::serde::encode_to_vec(&gap, cfg).unwrap();
        let (decoded, _): (MarkerGap, _) = bincode::serde::decode_from_slice(&bytes, cfg).unwrap();
        assert_eq!(gap, decoded);

        // StreamDictEntry
        let dict = StreamDictEntry {
            slot: 2,
            data_cls: DataClass::Bar,
            identifier: "BTCUSDT-PERP.BINANCE".to_string(),
        };
        let bytes = bincode::serde::encode_to_vec(&dict, cfg).unwrap();
        let (decoded, _): (StreamDictEntry, _) =
            bincode::serde::decode_from_slice(&bytes, cfg).unwrap();
        assert_eq!(dict, decoded);
    }

    #[rstest]
    #[case::quote_lowercase("quote")]
    #[case::empty("")]
    #[case::trailing_s("Quotes")]
    #[case::partial("BookDepth")]
    fn data_class_from_str_rejects_unknown(#[case] input: &str) {
        let err = DataClass::from_str(input).unwrap_err();

        assert!(
            err.contains(input),
            "error should name the rejected input, was `{err}`"
        );
    }

    #[rstest]
    #[case::marker_seq(|s: &mut DataCursorSnapshot| s.marker_seq = 99)]
    #[case::event_seq_before(|s: &mut DataCursorSnapshot| s.event_seq_before = 99)]
    #[case::ts_init(|s: &mut DataCursorSnapshot| s.ts_init = UnixNanos::from(1))]
    #[case::cursor_slot(|s: &mut DataCursorSnapshot| s.advanced[0].slot = 256)]
    #[case::cursor_ts_init_hi(|s: &mut DataCursorSnapshot| s.advanced[0].ts_init_hi = UnixNanos::from(1))]
    #[case::cursor_count(|s: &mut DataCursorSnapshot| s.advanced[0].count = 999)]
    #[case::extra_cursor(|s: &mut DataCursorSnapshot| s.advanced.push(StreamCursor { slot: 2, ts_init_hi: UnixNanos::from(1_700_000_000_000_000_003), count: 1 }))]
    #[case::cursor_order(|s: &mut DataCursorSnapshot| s.advanced.reverse())]
    fn every_marker_field_affects_hash(#[case] mutate: fn(&mut DataCursorSnapshot)) {
        let base = baseline_snapshot();
        let mut mutated = base.clone();
        mutate(&mut mutated);

        assert_ne!(compute_marker_hash(&base), compute_marker_hash(&mutated));
    }

    #[rstest]
    #[case::marker_seq(|m: &mut HiFiMarker| m.marker_seq = 99)]
    #[case::event_seq_before(|m: &mut HiFiMarker| m.event_seq_before = 99)]
    #[case::slot(|m: &mut HiFiMarker| m.slot = 256)]
    #[case::ts_event(|m: &mut HiFiMarker| m.ts_event = UnixNanos::from(1))]
    #[case::ts_init(|m: &mut HiFiMarker| m.ts_init = UnixNanos::from(1))]
    #[case::same_ts_ordinal(|m: &mut HiFiMarker| m.same_ts_ordinal = 256)]
    #[case::fingerprint(|m: &mut HiFiMarker| m.record_fingerprint[0] ^= 0x01)]
    fn every_hifi_field_affects_hash(#[case] mutate: fn(&mut HiFiMarker)) {
        let base = baseline_hifi();
        let mut mutated = base.clone();
        mutate(&mut mutated);

        assert_ne!(compute_hifi_hash(&base), compute_hifi_hash(&mutated));
    }

    #[rstest]
    fn marker_hash_handles_empty_advanced() {
        let empty = DataCursorSnapshot {
            marker_seq: 1,
            event_seq_before: 42,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000),
            advanced: vec![],
        };

        assert_eq!(compute_marker_hash(&empty), compute_marker_hash(&empty));
        assert_ne!(
            compute_marker_hash(&empty),
            compute_marker_hash(&baseline_snapshot())
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        // Any cursor snapshot survives a bincode encode/decode unchanged
        #[rstest]
        fn prop_marker_snapshot_bincode_roundtrip(
            marker_seq in any::<u64>(),
            event_seq_before in any::<u64>(),
            ts_init in any::<u64>(),
            cursors in proptest::collection::vec((any::<u32>(), any::<u64>(), any::<u64>()), 0..8),
        ) {
            let cfg = bincode::config::standard();
            let snap = DataCursorSnapshot {
                marker_seq,
                event_seq_before,
                ts_init: UnixNanos::from(ts_init),
                advanced: cursors
                    .into_iter()
                    .map(|(slot, hi, count)| StreamCursor {
                        slot,
                        ts_init_hi: UnixNanos::from(hi),
                        count,
                    })
                    .collect(),
            };

            let bytes = bincode::serde::encode_to_vec(&snap, cfg).expect("encode");
            let (decoded, _): (DataCursorSnapshot, _) =
                bincode::serde::decode_from_slice(&bytes, cfg).expect("decode");
            prop_assert_eq!(snap, decoded);
        }

        // Any high-fidelity marker survives a bincode encode/decode unchanged
        #[rstest]
        fn prop_hifi_marker_bincode_roundtrip(
            marker_seq in any::<u64>(),
            event_seq_before in any::<u64>(),
            slot in any::<u32>(),
            ts_event in any::<u64>(),
            ts_init in any::<u64>(),
            same_ts_ordinal in any::<u32>(),
            fingerprint in proptest::array::uniform32(any::<u8>()),
        ) {
            let cfg = bincode::config::standard();
            let marker = HiFiMarker {
                marker_seq,
                event_seq_before,
                slot,
                ts_event: UnixNanos::from(ts_event),
                ts_init: UnixNanos::from(ts_init),
                same_ts_ordinal,
                record_fingerprint: fingerprint,
            };

            let bytes = bincode::serde::encode_to_vec(&marker, cfg).expect("encode");
            let (decoded, _): (HiFiMarker, _) =
                bincode::serde::decode_from_slice(&bytes, cfg).expect("decode");
            prop_assert_eq!(marker, decoded);
        }
    }
}
