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

use core::cmp::Ordering;
use std::{collections::HashMap, str::FromStr};

use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::{
    http::models::{HyperliquidL2Book, HyperliquidLevel},
    websocket::messages::{WsBookData, WsLevelData},
};

/// Compact level tuple in integer grid units (ticks/steps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxQty {
    pub px: i64,
    pub qty: i64,
}

impl PxQty {
    #[inline]
    pub fn new(px: i64, qty: i64) -> Self {
        Self { px, qty }
    }
}

/// Error conditions from applying deltas/snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Sequence gap detected (resubscribe / REST snapshot required).
    Gap { expected: u64, received: u64 },
    /// Provided checksum (from feed) didn't match computed checksum.
    ChecksumMismatch { expected: u32, computed: u32 },
}

/// L2 order book (pure state).
#[derive(Debug, Default, Clone)]
pub struct L2Book {
    pub seq: u64,
    pub bids: Vec<PxQty>, // sorted DESC by price
    pub asks: Vec<PxQty>, // sorted ASC by price
    pub digest: u64,      // deterministic state hash (for assertions)
}

impl L2Book {
    /// Apply full snapshot (replaces state). Caller must pass canonicalized sides or rely on internal sort.
    pub fn apply_snapshot(
        &mut self,
        seq: u64,
        mut bids: Vec<PxQty>,
        mut asks: Vec<PxQty>,
        checksum: Option<u32>,
    ) -> Result<(), ApplyError> {
        sort_desc(&mut bids);
        sort_asc(&mut asks);

        // Assign then checksum/hash
        self.seq = seq;
        self.bids = bids;
        self.asks = asks;

        if let Some(exp) = checksum {
            let got = compute_checksum32(&self.bids, &self.asks, self.seq);
            if exp != got {
                return Err(ApplyError::ChecksumMismatch {
                    expected: exp,
                    computed: got,
                });
            }
        }

        self.digest = compute_digest64(&self.bids, &self.asks, self.seq);
        Ok(())
    }

    /// Apply delta (upserts/removals). Returns Gap if `next_seq != prev_seq + 1`.
    /// If a checksum is provided it is verified *after* the delta is applied.
    pub fn apply_delta(
        &mut self,
        next_seq: u64,
        upserts_bids: &[PxQty],
        upserts_asks: &[PxQty],
        removals_bids: &[i64],
        removals_asks: &[i64],
        checksum: Option<u32>,
    ) -> Result<(), ApplyError> {
        // Gap detection (seq=0 is "uninitialized"; allow any next_seq as first delta only if empty).
        if self.seq != 0 {
            let expected = self.seq + 1;
            if next_seq != expected {
                return Err(ApplyError::Gap {
                    expected,
                    received: next_seq,
                });
            }
        }

        // Apply removals first, then upserts (so an upsert can "revive" a removed price).
        for p in removals_bids {
            remove_level(&mut self.bids, *p, /*desc=*/ true);
        }
        for p in removals_asks {
            remove_level(&mut self.asks, *p, /*desc=*/ false);
        }

        for PxQty { px, qty } in upserts_bids {
            if *qty == 0 {
                remove_level(&mut self.bids, *px, /*desc=*/ true);
            } else {
                upsert_level(&mut self.bids, *px, *qty, /*desc=*/ true);
            }
        }
        for PxQty { px, qty } in upserts_asks {
            if *qty == 0 {
                remove_level(&mut self.asks, *px, /*desc=*/ false);
            } else {
                upsert_level(&mut self.asks, *px, *qty, /*desc=*/ false);
            }
        }

        self.seq = next_seq;

        if let Some(exp) = checksum {
            let got = compute_checksum32(&self.bids, &self.asks, self.seq);
            if exp != got {
                return Err(ApplyError::ChecksumMismatch {
                    expected: exp,
                    computed: got,
                });
            }
        }

        self.digest = compute_digest64(&self.bids, &self.asks, self.seq);
        Ok(())
    }
}

/* ===== Implementation helpers (pure, deterministic) ===== */

#[inline]
fn sort_desc(levels: &mut [PxQty]) {
    levels.sort_unstable_by(|a, b| b.px.cmp(&a.px));
}

#[inline]
fn sort_asc(levels: &mut [PxQty]) {
    levels.sort_unstable_by(|a, b| a.px.cmp(&b.px));
}

#[inline]
fn binsearch_idx(levels: &[PxQty], price: i64, desc: bool) -> Result<usize, usize> {
    if desc {
        // levels sorted by px DESC — search by reversed order
        levels.binary_search_by(|e| cmp_desc(e.px, price))
    } else {
        levels.binary_search_by(|e| e.px.cmp(&price))
    }
}

#[inline]
fn cmp_desc(a: i64, b: i64) -> Ordering {
    // Compare element 'a' vs target 'b' in DESC order.
    // For binary_search_by, closure compares element to target.
    b.cmp(&a)
}

#[inline]
fn remove_level(levels: &mut Vec<PxQty>, price: i64, desc: bool) {
    if let Ok(ix) = binsearch_idx(levels, price, desc) {
        levels.remove(ix);
    }
}

#[inline]
fn upsert_level(levels: &mut Vec<PxQty>, price: i64, qty: i64, desc: bool) {
    match binsearch_idx(levels, price, desc) {
        Ok(ix) => levels[ix].qty = qty,
        Err(ix) => levels.insert(ix, PxQty::new(price, qty)),
    }
}

/// Deterministic 32-bit checksum (FNV-1a) over (bids, asks, seq).
/// Note: This is *not* HL's checksum; it is an internal assertion tool.
pub fn compute_checksum32(bids: &[PxQty], asks: &[PxQty], seq: u64) -> u32 {
    let mut h: u32 = 0x811c9dc5; // FNV-1a 32 offset
    const P: u32 = 0x0100_0193;

    #[inline]
    fn upd(mut h: u32, b: u8) -> u32 {
        h ^= b as u32;
        h = h.wrapping_mul(P);
        h
    }

    // tag sides to avoid collisions
    for PxQty { px, qty } in bids {
        h = upd(h, b'B');
        for b in px.to_be_bytes() {
            h = upd(h, b);
        }
        for b in qty.to_be_bytes() {
            h = upd(h, b);
        }
    }
    for PxQty { px, qty } in asks {
        h = upd(h, b'A');
        for b in px.to_be_bytes() {
            h = upd(h, b);
        }
        for b in qty.to_be_bytes() {
            h = upd(h, b);
        }
    }
    for b in seq.to_be_bytes() {
        h = upd(h, b);
    }
    h
}

/// Deterministic 64-bit digest (FNV-1a) over (bids, asks, seq).
pub fn compute_digest64(bids: &[PxQty], asks: &[PxQty], seq: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    const P: u64 = 0x0000_0001_0000_01b3;

    #[inline]
    fn upd(mut h: u64, b: u8) -> u64 {
        h ^= b as u64;
        h = h.wrapping_mul(P);
        h
    }

    for PxQty { px, qty } in bids {
        h = upd(h, b'B');
        for b in px.to_be_bytes() {
            h = upd(h, b);
        }
        for b in qty.to_be_bytes() {
            h = upd(h, b);
        }
    }
    for PxQty { px, qty } in asks {
        h = upd(h, b'A');
        for b in px.to_be_bytes() {
            h = upd(h, b);
        }
        for b in qty.to_be_bytes() {
            h = upd(h, b);
        }
    }
    for b in seq.to_be_bytes() {
        h = upd(h, b);
    }
    h
}

/* ===== Hyperliquid Integration (Book Manager) ===== */

/// Manages orderbooks for multiple coins and handles conversions
#[derive(Debug, Default)]
pub struct HyperliquidBookManager {
    /// Active orderbooks by coin symbol
    books: HashMap<String, L2Book>,
    /// Price multipliers for converting to integer ticks (coin -> multiplier)
    price_multipliers: HashMap<String, i64>,
    /// Size multipliers for converting to integer ticks (coin -> multiplier)
    size_multipliers: HashMap<String, i64>,
}

/// Configuration for price/size conversion
#[derive(Debug, Clone)]
pub struct BookConfig {
    /// Price precision (number of decimal places)
    pub price_decimals: u32,
    /// Size precision (number of decimal places)
    pub size_decimals: u32,
}

impl BookConfig {
    /// Create config with standard precision
    pub fn new(price_decimals: u32, size_decimals: u32) -> Self {
        Self {
            price_decimals,
            size_decimals,
        }
    }

    /// Get price multiplier for converting to integer ticks
    pub fn price_multiplier(&self) -> i64 {
        10_i64.pow(self.price_decimals)
    }

    /// Get size multiplier for converting to integer ticks
    pub fn size_multiplier(&self) -> i64 {
        10_i64.pow(self.size_decimals)
    }
}

impl HyperliquidBookManager {
    /// Create a new book manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure a coin with specific precision settings
    pub fn configure_coin(&mut self, coin: &str, config: BookConfig) {
        self.price_multipliers
            .insert(coin.to_string(), config.price_multiplier());
        self.size_multipliers
            .insert(coin.to_string(), config.size_multiplier());
    }

    /// Get or create orderbook for a coin
    pub fn get_book(&mut self, coin: &str) -> &mut L2Book {
        self.books.entry(coin.to_string()).or_default()
    }

    /// Get read-only access to a book
    pub fn get_book_readonly(&self, coin: &str) -> Option<&L2Book> {
        self.books.get(coin)
    }

    /// Convert Hyperliquid HTTP L2Book to our orderbook snapshot
    pub fn apply_http_snapshot(
        &mut self,
        data: &HyperliquidL2Book,
        config: Option<BookConfig>,
    ) -> Result<(), ConversionError> {
        // Configure coin if not already done
        if !self.price_multipliers.contains_key(&data.coin) {
            let cfg = config.unwrap_or_else(|| BookConfig::new(2, 5)); // Default: 0.01 price, 0.00001 size
            self.configure_coin(&data.coin, cfg);
        }

        let price_mult = *self.price_multipliers.get(&data.coin).ok_or_else(|| {
            ConversionError::ConfigMissing {
                coin: data.coin.clone(),
            }
        })?;
        let size_mult = *self.size_multipliers.get(&data.coin).ok_or_else(|| {
            ConversionError::ConfigMissing {
                coin: data.coin.clone(),
            }
        })?;

        // Convert levels
        let bids = convert_levels(&data.levels[0], price_mult, size_mult)?;
        let asks = convert_levels(&data.levels[1], price_mult, size_mult)?;

        // Apply to orderbook (use timestamp as sequence for HTTP snapshots)
        let book = self.get_book(&data.coin);
        book.apply_snapshot(data.time, bids, asks, None)
            .map_err(ConversionError::ApplyError)
    }

    /// Convert WebSocket book data to orderbook update
    pub fn apply_ws_snapshot(
        &mut self,
        data: &WsBookData,
        config: Option<BookConfig>,
    ) -> Result<(), ConversionError> {
        // Configure coin if not already done
        if !self.price_multipliers.contains_key(&data.coin) {
            let cfg = config.unwrap_or_else(|| BookConfig::new(2, 5)); // Default: 0.01 price, 0.00001 size
            self.configure_coin(&data.coin, cfg);
        }

        let price_mult = *self.price_multipliers.get(&data.coin).ok_or_else(|| {
            ConversionError::ConfigMissing {
                coin: data.coin.clone(),
            }
        })?;
        let size_mult = *self.size_multipliers.get(&data.coin).ok_or_else(|| {
            ConversionError::ConfigMissing {
                coin: data.coin.clone(),
            }
        })?;

        // Convert levels
        let bids = convert_ws_levels(&data.levels[0], price_mult, size_mult)?;
        let asks = convert_ws_levels(&data.levels[1], price_mult, size_mult)?;

        // Apply to orderbook (use timestamp as sequence for WS snapshots)
        let book = self.get_book(&data.coin);
        book.apply_snapshot(data.time, bids, asks, None)
            .map_err(ConversionError::ApplyError)
    }

    /// Get best bid/ask for a coin
    pub fn get_best_bid_ask(&self, coin: &str) -> Option<(Option<PxQty>, Option<PxQty>)> {
        let book = self.get_book_readonly(coin)?;
        let best_bid = book.bids.first().copied();
        let best_ask = book.asks.first().copied();
        Some((best_bid, best_ask))
    }

    /// Convert integer ticks back to decimal price
    pub fn ticks_to_price(&self, coin: &str, ticks: i64) -> Option<Decimal> {
        let multiplier = *self.price_multipliers.get(coin)?;
        Some(Decimal::from(ticks) / Decimal::from(multiplier))
    }

    /// Convert integer ticks back to decimal size
    pub fn ticks_to_size(&self, coin: &str, ticks: i64) -> Option<Decimal> {
        let multiplier = *self.size_multipliers.get(coin)?;
        Some(Decimal::from(ticks) / Decimal::from(multiplier))
    }
}

/// Convert HTTP levels to PxQty vector.
#[inline]
fn convert_levels(
    levels: &[HyperliquidLevel],
    price_mult: i64,
    size_mult: i64,
) -> Result<Vec<PxQty>, ConversionError> {
    levels
        .iter()
        .map(|level| {
            let price_decimal =
                Decimal::from_str(&level.px).map_err(|_| ConversionError::InvalidPrice {
                    value: level.px.clone(),
                })?;
            let size_decimal =
                Decimal::from_str(&level.sz).map_err(|_| ConversionError::InvalidSize {
                    value: level.sz.clone(),
                })?;

            let price_ticks = (price_decimal * Decimal::from(price_mult))
                .to_i64()
                .ok_or_else(|| ConversionError::PriceOverflow {
                    value: level.px.clone(),
                })?;
            let size_ticks = (size_decimal * Decimal::from(size_mult))
                .to_i64()
                .ok_or_else(|| ConversionError::SizeOverflow {
                    value: level.sz.clone(),
                })?;

            Ok(PxQty::new(price_ticks, size_ticks))
        })
        .collect()
}

/// Convert WebSocket levels to PxQty vector.
#[inline]
fn convert_ws_levels(
    levels: &[WsLevelData],
    price_mult: i64,
    size_mult: i64,
) -> Result<Vec<PxQty>, ConversionError> {
    levels
        .iter()
        .map(|level| {
            let price_decimal =
                Decimal::from_str(&level.px).map_err(|_| ConversionError::InvalidPrice {
                    value: level.px.clone(),
                })?;
            let size_decimal =
                Decimal::from_str(&level.sz).map_err(|_| ConversionError::InvalidSize {
                    value: level.sz.clone(),
                })?;

            let price_ticks = (price_decimal * Decimal::from(price_mult))
                .to_i64()
                .ok_or_else(|| ConversionError::PriceOverflow {
                    value: level.px.clone(),
                })?;
            let size_ticks = (size_decimal * Decimal::from(size_mult))
                .to_i64()
                .ok_or_else(|| ConversionError::SizeOverflow {
                    value: level.sz.clone(),
                })?;

            Ok(PxQty::new(price_ticks, size_ticks))
        })
        .collect()
}

/// Error conditions from Hyperliquid data conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    /// Missing configuration for coin (resubscribe required).
    ConfigMissing { coin: String },
    /// Invalid price string format.
    InvalidPrice { value: String },
    /// Invalid size string format.
    InvalidSize { value: String },
    /// Price value overflow (exceeds i64 range).
    PriceOverflow { value: String },
    /// Size value overflow (exceeds i64 range).
    SizeOverflow { value: String },
    /// Error applying to orderbook core.
    ApplyError(ApplyError),
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::ConfigMissing { coin } => {
                write!(f, "Missing configuration for coin: {}", coin)
            }
            ConversionError::InvalidPrice { value } => write!(f, "Invalid price: {}", value),
            ConversionError::InvalidSize { value } => write!(f, "Invalid size: {}", value),
            ConversionError::PriceOverflow { value } => write!(f, "Price overflow: {}", value),
            ConversionError::SizeOverflow { value } => write!(f, "Size overflow: {}", value),
            ConversionError::ApplyError(err) => write!(f, "Apply error: {:?}", err),
        }
    }
}

impl std::error::Error for ConversionError {}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn mk_levels(range: core::ops::RangeInclusive<i64>, qty: i64) -> Vec<PxQty> {
        range.map(|p| PxQty::new(p, qty)).collect()
    }

    #[rstest]
    fn snapshot_establishes_canonical_sort_and_digest() {
        let mut book = L2Book::default();
        // Unsorted input to ensure we canonicalize.
        let bids = vec![PxQty::new(101, 5), PxQty::new(103, 3), PxQty::new(102, 4)];
        let asks = vec![PxQty::new(106, 7), PxQty::new(104, 9), PxQty::new(105, 1)];

        book.apply_snapshot(10, bids, asks, /*checksum*/ None)
            .unwrap();

        // Bids DESC
        assert_eq!(
            book.bids.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![103, 102, 101]
        );
        // Asks ASC
        assert_eq!(
            book.asks.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![104, 105, 106]
        );
        // Digest stable and non-zero
        assert_ne!(book.digest, 0);
    }

    #[rstest]
    fn delta_upserts_and_removals_preserve_order_and_hash() {
        let mut book = L2Book::default();
        // Snapshot
        book.apply_snapshot(
            1,
            mk_levels(100..=102, 10), // bids 100..102 (we will sort DESC)
            mk_levels(103..=105, 20), // asks 103..105 (ASC)
            None,
        )
        .unwrap();

        let digest0 = book.digest;

        // Remove best ask @103, upsert bid @101 -> qty 15, and add a new ask @106 qty 7.
        book.apply_delta(
            2,
            &[PxQty::new(101, 15)], // upserts_bids
            &[PxQty::new(106, 7)],  // upserts_asks
            &[],                    // removals_bids
            &[103],                 // removals_asks
            None,
        )
        .unwrap();

        // Check ordering
        assert_eq!(
            book.bids.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![102, 101, 100]
        );
        assert_eq!(
            book.asks.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![104, 105, 106]
        );

        // Hash changed
        assert_ne!(book.digest, digest0);
    }

    #[rstest]
    fn zero_qty_upsert_equals_removal() {
        let mut book = L2Book::default();
        book.apply_snapshot(5, mk_levels(100..=100, 10), mk_levels(101..=101, 20), None)
            .unwrap();
        assert_eq!(book.bids.len(), 1);

        book.apply_delta(6, &[PxQty::new(100, 0)], &[], &[], &[], None)
            .unwrap();
        assert!(book.bids.is_empty(), "qty=0 upsert should remove level");
    }

    #[rstest]
    fn gap_detection_and_resync_via_snapshot() {
        let mut book = L2Book::default();
        book.apply_snapshot(10, mk_levels(100..=101, 5), mk_levels(102..=103, 7), None)
            .unwrap();

        // Seq 11 OK
        book.apply_delta(11, &[PxQty::new(100, 6)], &[], &[], &[], None)
            .unwrap();

        // Jump to 13 → gap (expected 12)
        let err = book.apply_delta(13, &[], &[], &[], &[], None).unwrap_err();
        assert_eq!(
            err,
            ApplyError::Gap {
                expected: 12,
                received: 13
            }
        );

        // Resync via snapshot (arbitrary fresh state), then continue
        book.apply_snapshot(20, mk_levels(200..=200, 1), mk_levels(201..=201, 1), None)
            .unwrap();
        book.apply_delta(21, &[PxQty::new(200, 2)], &[], &[], &[], None)
            .unwrap();
        assert_eq!(book.seq, 21);
    }

    #[rstest]
    fn checksum_mismatch_is_detected_on_snapshot_and_delta() {
        let mut book = L2Book::default();

        // Prepare a snapshot and compute its checksum, then corrupt expected
        let bids = mk_levels(100..=100, 10);
        let asks = mk_levels(101..=101, 20);
        let seq = 1;
        let good = compute_checksum32(&bids, &asks, seq);
        let bad = good ^ 0xdead_beef; // different

        // Mismatch should error
        let err = book
            .apply_snapshot(seq, bids.clone(), asks.clone(), Some(bad))
            .unwrap_err();
        assert!(matches!(err, ApplyError::ChecksumMismatch { .. }));

        // Apply good snapshot
        book.apply_snapshot(seq, bids, asks, Some(good)).unwrap();

        // Now a delta with wrong checksum
        let err2 = book
            .apply_delta(2, &[PxQty::new(100, 11)], &[], &[], &[], Some(0x1234_5678))
            .unwrap_err();
        assert!(matches!(err2, ApplyError::ChecksumMismatch { .. }));
    }

    #[rstest]
    fn deterministic_replay_yields_identical_digest() {
        // Construct a small stream (snapshot + deltas). Apply twice to two books → same digest.
        let snapshot_bids = vec![PxQty::new(100, 10), PxQty::new(99, 5)];
        let snapshot_asks = vec![PxQty::new(101, 7), PxQty::new(102, 9)];

        // Stream A (single deltas)
        let mut a = L2Book::default();
        a.apply_snapshot(100, snapshot_bids.clone(), snapshot_asks.clone(), None)
            .unwrap();
        a.apply_delta(101, &[PxQty::new(99, 6)], &[], &[], &[], None)
            .unwrap();
        a.apply_delta(102, &[], &[PxQty::new(103, 3)], &[], &[], None)
            .unwrap();
        a.apply_delta(103, &[], &[], &[99], &[], None).unwrap();

        // Stream B (equivalent changes packed differently)
        let mut b = L2Book::default();
        b.apply_snapshot(100, snapshot_bids, snapshot_asks, None)
            .unwrap();
        // Combine two ops into one delta that yields the same end state.
        b.apply_delta(
            101,
            &[PxQty::new(99, 6)], // upsert same as A@101
            &[],
            &[],
            &[],
            None,
        )
        .unwrap();
        b.apply_delta(102, &[], &[PxQty::new(103, 3)], &[], &[], None)
            .unwrap();
        b.apply_delta(103, &[], &[], &[99], &[], None).unwrap();

        assert_eq!(a.seq, b.seq);
        assert_eq!(a.bids, b.bids);
        assert_eq!(a.asks, b.asks);
        assert_eq!(
            a.digest, b.digest,
            "end digests must match for identical logical state"
        );
    }

    #[rstest]
    fn binary_search_insert_positions_are_correct_for_desc_and_asc() {
        // Bids: DESC
        let mut bids = vec![PxQty::new(105, 1), PxQty::new(103, 1), PxQty::new(100, 1)];
        // Insert 104 between 105 and 103
        upsert_level(&mut bids, 104, 2, /*desc=*/ true);
        assert_eq!(
            bids.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![105, 104, 103, 100]
        );

        // Asks: ASC
        let mut asks = vec![PxQty::new(100, 1), PxQty::new(102, 1), PxQty::new(105, 1)];
        // Insert 101 between 100 and 102
        upsert_level(&mut asks, 101, 3, /*desc=*/ false);
        assert_eq!(
            asks.iter().map(|l| l.px).collect::<Vec<_>>(),
            vec![100, 101, 102, 105]
        );
    }

    // Book Manager Tests
    fn sample_http_book() -> HyperliquidL2Book {
        HyperliquidL2Book {
            coin: "BTC".to_string(),
            levels: vec![
                vec![
                    HyperliquidLevel {
                        px: "50000.00".to_string(),
                        sz: "1.5".to_string(),
                    },
                    HyperliquidLevel {
                        px: "49999.50".to_string(),
                        sz: "2.0".to_string(),
                    },
                ],
                vec![
                    HyperliquidLevel {
                        px: "50001.00".to_string(),
                        sz: "1.0".to_string(),
                    },
                    HyperliquidLevel {
                        px: "50002.50".to_string(),
                        sz: "3.0".to_string(),
                    },
                ],
            ],
            time: 1234567890,
        }
    }

    #[rstest]
    fn test_http_book_conversion() {
        let mut manager = HyperliquidBookManager::new();
        let book_data = sample_http_book();
        let config = BookConfig::new(2, 5); // 0.01 price tick, 0.00001 size tick

        // Apply snapshot
        manager
            .apply_http_snapshot(&book_data, Some(config))
            .unwrap();

        // Verify conversion
        let book = manager.get_book_readonly("BTC").unwrap();
        assert_eq!(book.seq, 1234567890);
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks.len(), 2);

        // Check price conversion (50000.00 * 100 = 5000000)
        assert_eq!(book.bids[0].px, 5000000);
        assert_eq!(book.bids[0].qty, 150000); // 1.5 * 100000

        // Check ordering (bids should be DESC)
        assert!(book.bids[0].px > book.bids[1].px);
        // Check ordering (asks should be ASC)
        assert!(book.asks[0].px < book.asks[1].px);
    }

    #[rstest]
    fn test_price_size_conversion() {
        let mut manager = HyperliquidBookManager::new();
        let config = BookConfig::new(2, 5);
        manager.configure_coin("BTC", config);

        // Test tick to price conversion
        let price = manager.ticks_to_price("BTC", 5000000).unwrap();
        assert_eq!(price, Decimal::from(50000));

        // Test tick to size conversion
        let size = manager.ticks_to_size("BTC", 150000).unwrap();
        assert_eq!(size, Decimal::new(15, 1)); // 1.5
    }

    #[rstest]
    fn test_best_bid_ask() {
        let mut manager = HyperliquidBookManager::new();
        let book_data = sample_http_book();
        let config = BookConfig::new(2, 5);

        manager
            .apply_http_snapshot(&book_data, Some(config))
            .unwrap();

        let (best_bid, best_ask) = manager.get_best_bid_ask("BTC").unwrap();
        assert!(best_bid.is_some());
        assert!(best_ask.is_some());

        let bid = best_bid.unwrap();
        let ask = best_ask.unwrap();

        // Best bid should be highest price
        assert_eq!(bid.px, 5000000); // 50000.00
        // Best ask should be lowest price
        assert_eq!(ask.px, 5000100); // 50001.00
    }
}
