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

//! WebSocket message handler with sequence tracking and orderbook reconstruction.
//!
//! ## Orderbook YES/NO Duality
//!
//! Kalshi only exposes bids. The YES/NO relationship is:
//! - YES bid at price X → occupies the bid side
//! - NO bid at price Y → equivalent to YES ask at `1.00 - Y`
//!
//! This handler converts NO bids into YES asks so NautilusTrader sees a
//! standard bid/ask orderbook on the YES side.

use std::collections::HashMap;
use std::str::FromStr;

use log::{debug, warn};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::websocket::{
    error::KalshiWsError,
    messages::{KalshiWsMessage, KalshiWsOrderbookDelta, KalshiWsOrderbookSnapshot},
};

/// Tracks sequence numbers per subscription to detect gaps.
#[derive(Debug, Default)]
pub struct SequenceTracker {
    /// `sid` → last seen `seq`.
    last_seq: HashMap<u32, u64>,
}

impl SequenceTracker {
    /// Validate a sequence number for a subscription.
    ///
    /// Returns `Ok(())` if in order, `Err(KalshiWsError::SequenceGap)` if a gap is detected.
    pub fn check(&mut self, sid: u32, seq: u64) -> Result<(), KalshiWsError> {
        let entry = self.last_seq.entry(sid).or_insert(0);
        if *entry == 0 {
            // First message on this subscription — accept any seq.
            *entry = seq;
            return Ok(());
        }
        if seq != *entry + 1 {
            return Err(KalshiWsError::SequenceGap {
                sid,
                expected: *entry + 1,
                got: seq,
            });
        }
        *entry = seq;
        Ok(())
    }

    /// Reset tracking for a subscription (e.g. after re-subscribe).
    pub fn reset(&mut self, sid: u32) {
        self.last_seq.remove(&sid);
    }
}

/// Holds the in-memory orderbook for a single Kalshi market.
///
/// Prices are stored as 4-decimal-place strings to avoid float precision issues.
/// YES bids and derived YES asks (from NO bids) are maintained separately.
#[derive(Debug, Default)]
pub struct KalshiOrderBook {
    /// YES bids: price_str → quantity_str
    pub yes_bids: HashMap<String, String>,
    /// YES asks derived from NO bids: price_str → quantity_str
    pub yes_asks: HashMap<String, String>,
}

impl KalshiOrderBook {
    /// Apply an orderbook snapshot, replacing all existing levels.
    pub fn apply_snapshot(&mut self, snapshot: &KalshiWsOrderbookSnapshot) {
        self.yes_bids.clear();
        self.yes_asks.clear();

        for (price, qty) in &snapshot.yes_dollars_fp {
            self.yes_bids.insert(price.clone(), qty.clone());
        }
        for (no_price, qty) in &snapshot.no_dollars_fp {
            // Convert NO bid at Y to YES ask at (1.0000 - Y).
            if let Some(yes_ask_price) = complement_price(no_price) {
                self.yes_asks.insert(yes_ask_price, qty.clone());
            }
        }
    }

    /// Apply a single delta update.
    pub fn apply_delta(&mut self, delta: &KalshiWsOrderbookDelta) {
        match delta.side.as_str() {
            "yes" => apply_level(&mut self.yes_bids, &delta.price_dollars, &delta.delta_fp),
            "no" => {
                if let Some(yes_ask_price) = complement_price(&delta.price_dollars) {
                    apply_level(&mut self.yes_asks, &yes_ask_price, &delta.delta_fp);
                }
            }
            other => warn!("Kalshi: unknown orderbook side '{other}'"),
        }
    }
}

/// Convert a NO bid price to the equivalent YES ask price: `1.0000 - no_price`.
fn complement_price(no_price: &str) -> Option<String> {
    let p: Decimal = Decimal::from_str(no_price).ok()?;
    let one = Decimal::ONE;
    let ask = one - p;
    // Format to 4 decimal places matching Kalshi's precision.
    Some(format!("{ask:.4}"))
}

/// Apply a quantity delta to a price level map.
///
/// - Positive delta: add to existing quantity (or insert new level).
/// - Negative delta: subtract from quantity.
/// - Zero delta or quantity reaches zero: remove the level.
fn apply_level(levels: &mut HashMap<String, String>, price: &str, delta_fp: &str) {
    let delta = match Decimal::from_str(delta_fp) {
        Ok(d) => d,
        Err(e) => {
            warn!("Kalshi: invalid delta_fp '{delta_fp}': {e}");
            return;
        }
    };

    if delta == Decimal::ZERO {
        levels.remove(price);
        return;
    }

    let existing = levels
        .get(price)
        .and_then(|q| Decimal::from_str(q).ok())
        .unwrap_or(Decimal::ZERO);

    let new_qty = existing + delta;
    if new_qty <= Decimal::ZERO {
        levels.remove(price);
    } else {
        levels.insert(price.to_string(), format!("{new_qty:.2}"));
    }
}

/// Top-level message handler: dispatches messages, tracks sequence numbers,
/// and updates in-memory orderbooks.
///
/// On sequence gap, returns `Err(KalshiWsError::SequenceGap)` — the caller
/// should re-subscribe to get a fresh snapshot.
#[derive(Debug, Default)]
pub struct KalshiWsHandler {
    pub seq_tracker: SequenceTracker,
    /// Per-market orderbook state.
    pub books: HashMap<Ustr, KalshiOrderBook>,
}

impl KalshiWsHandler {
    /// Process one raw JSON message from the WebSocket.
    ///
    /// Returns the parsed message on success, or an error on sequence gap or parse failure.
    pub fn handle(&mut self, raw: &str) -> Result<KalshiWsMessage, KalshiWsError> {
        let msg = KalshiWsMessage::from_json(raw)?;

        match &msg {
            KalshiWsMessage::OrderbookSnapshot { sid, seq, data } => {
                // Snapshots reset the sequence for this subscription.
                self.seq_tracker.reset(*sid);
                self.seq_tracker.check(*sid, *seq)?;
                let book = self.books.entry(data.market_ticker).or_default();
                book.apply_snapshot(data);
                debug!("Kalshi: snapshot applied for {}", data.market_ticker);
            }
            KalshiWsMessage::OrderbookDelta { sid, seq, data } => {
                self.seq_tracker.check(*sid, *seq)?;
                if let Some(book) = self.books.get_mut(&data.market_ticker) {
                    book.apply_delta(data);
                } else {
                    warn!("Kalshi: delta before snapshot for {}", data.market_ticker);
                }
            }
            KalshiWsMessage::Trade { sid, seq, .. } => {
                self.seq_tracker.check(*sid, *seq)?;
            }
            KalshiWsMessage::Error(e) => {
                warn!("Kalshi WS error {}: {}", e.code, e.msg);
            }
            KalshiWsMessage::Unknown(t) => {
                debug!("Kalshi: unknown WS message type '{t}'");
            }
        }

        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing: {name}"))
    }

    #[test]
    fn test_snapshot_populates_book() {
        let mut handler = KalshiWsHandler::default();
        handler
            .handle(&load_fixture("ws_orderbook_snapshot.json"))
            .unwrap();
        let ticker = Ustr::from("KXBTC-25MAR15-B100000");
        let book = handler.books.get(&ticker).unwrap();
        assert!(book.yes_bids.contains_key("0.4200"));
        // NO bid at 0.5600 → YES ask at 0.4400
        assert!(book.yes_asks.contains_key("0.4400"));
    }

    #[test]
    fn test_delta_updates_book() {
        let mut handler = KalshiWsHandler::default();
        handler
            .handle(&load_fixture("ws_orderbook_snapshot.json"))
            .unwrap();
        handler
            .handle(&load_fixture("ws_orderbook_delta.json"))
            .unwrap();
        let ticker = Ustr::from("KXBTC-25MAR15-B100000");
        let book = handler.books.get(&ticker).unwrap();
        // 13.00 + 50.00 = 63.00
        assert_eq!(
            book.yes_bids.get("0.4200").map(String::as_str),
            Some("63.00")
        );
    }

    #[test]
    fn test_sequence_gap_returns_error() {
        let mut tracker = SequenceTracker::default();
        tracker.check(1, 1).unwrap();
        tracker.check(1, 2).unwrap();
        // Gap: skipped seq 3.
        let err = tracker.check(1, 4).unwrap_err();
        assert!(matches!(
            err,
            KalshiWsError::SequenceGap {
                sid: 1,
                expected: 3,
                got: 4
            }
        ));
    }

    #[test]
    fn test_complement_price() {
        assert_eq!(complement_price("0.5600"), Some("0.4400".to_string()));
        assert_eq!(complement_price("0.5400"), Some("0.4600".to_string()));
    }
}
