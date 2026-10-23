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

//! Validates incremental book hash groups.
//!
//! With verified snapshot metadata, frames sharing a timestamp and hash emit one normalized batch
//! only after the book hash matches. Without verified metadata, updates pass through unverified.
//!
//! Unresolved groups need a fresh snapshot if their timestamp or hash changes, a positive timeout
//! expires, or they exceed 4,096 deltas. Valid delayed bursts can also require recovery.
//!
//! Hash metadata retains snapshot spelling; trades do not update it. Mismatches can reflect metadata
//! changes or missing levels, so recovery does not prove book corruption.

use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::OrderBookDeltas,
    enums::RecordFlag,
    orderbook::{BookLevel, OrderBook},
};
use tokio_util::sync::CancellationToken;

use crate::websocket::{
    messages::{PolymarketBookLevel, PolymarketBookSnapshot},
    parse::book_snapshot_hash,
};

const MAX_PENDING_DELTAS: usize = 4096;

#[derive(Debug, Default)]
pub(crate) struct BookIntegrity {
    latest_timestamp: UnixNanos,
    context: Option<PolymarketBookSnapshot>,
    pending: Option<PendingHash>,
    pub(crate) monitor_running: bool,
    pub(crate) cancellation: CancellationToken,
}

impl BookIntegrity {
    pub(crate) fn observe(&mut self, timestamp: UnixNanos) -> bool {
        if self.is_stale(timestamp) {
            return false;
        }

        self.latest_timestamp = timestamp;
        true
    }

    pub(crate) fn is_stale(&self, timestamp: UnixNanos) -> bool {
        timestamp < self.latest_timestamp
    }

    pub(crate) fn resolve_snapshot(
        &self,
        mut snapshot: PolymarketBookSnapshot,
        min_order_size: Option<&str>,
        neg_risk: Option<bool>,
    ) -> PolymarketBookSnapshot {
        snapshot.min_order_size = snapshot
            .min_order_size
            .or_else(|| min_order_size.map(str::to_string));
        snapshot.neg_risk = snapshot.neg_risk.or(neg_risk);
        if let Some(context) = &self.context {
            snapshot.tick_size = snapshot.tick_size.or_else(|| context.tick_size.clone());
            snapshot.last_trade_price = snapshot
                .last_trade_price
                .or_else(|| context.last_trade_price.clone());
        }

        snapshot
    }

    pub(crate) fn accept_snapshot(&mut self, mut snapshot: PolymarketBookSnapshot, verified: bool) {
        self.pending = None;
        snapshot.bids.clear();
        snapshot.asks.clear();
        self.context = verified.then_some(snapshot);
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn requires_verification(&self) -> bool {
        self.context.is_some()
    }

    pub(crate) fn check_group(&self, timestamp: UnixNanos, hash: &str) -> anyhow::Result<()> {
        if let Some(pending) = &self.pending
            && (pending.deltas.ts_event != timestamp
                || pending.hash != hash
                || pending
                    .deadline
                    .is_some_and(|deadline| deadline <= Instant::now()))
        {
            anyhow::bail!("Previous book hash group could not be verified");
        }

        Ok(())
    }

    pub(crate) fn validate(
        &mut self,
        book: &OrderBook,
        deltas: OrderBookDeltas,
        timestamp: &str,
        hash: &str,
        timeout: Duration,
    ) -> anyhow::Result<Option<OrderBookDeltas>> {
        let Some(context) = &self.context else {
            return Ok(Some(deltas));
        };

        if hash.is_empty() {
            anyhow::bail!("Missing incremental book hash");
        }

        let pending_len = self
            .pending
            .as_ref()
            .map_or(0, |pending| pending.deltas.deltas.len());

        if pending_len.saturating_add(deltas.deltas.len()) > MAX_PENDING_DELTAS {
            anyhow::bail!("Unverified book hash group exceeds {MAX_PENDING_DELTAS} deltas");
        }

        let mut snapshot = context.clone();
        snapshot.timestamp = timestamp.to_string();
        snapshot.hash = Some(hash.to_string());
        snapshot.bids = hash_levels(book.bids(None));
        snapshot.asks = hash_levels(book.asks(None));

        if let Some(pending) = &mut self.pending {
            if let Some(last) = pending.deltas.deltas.last_mut() {
                last.flags &= !(RecordFlag::F_LAST as u8);
            }

            pending.deltas.deltas.extend(deltas.deltas);

            pending.deltas = OrderBookDeltas::new(
                book.instrument_id,
                std::mem::take(&mut pending.deltas.deltas),
            );
        } else {
            self.pending = Some(PendingHash {
                hash: hash.to_string(),
                deltas,
                deadline: (!timeout.is_zero()).then(|| Instant::now() + timeout),
            });
        }

        if book_snapshot_hash(&snapshot, None, None)?.as_deref() == Some(hash) {
            return Ok(self.pending.take().map(|pending| pending.deltas));
        }

        Ok(None)
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().and_then(|pending| pending.deadline)
    }

    pub(crate) fn discard_pending(&mut self) {
        self.pending = None;
    }
}

impl Drop for BookIntegrity {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
struct PendingHash {
    hash: String,
    deltas: OrderBookDeltas,
    deadline: Option<Instant>,
}

fn hash_levels<'a>(levels: impl Iterator<Item = &'a BookLevel>) -> Vec<PolymarketBookLevel> {
    let mut levels: Vec<_> = levels
        .map(|level| PolymarketBookLevel {
            price: level.price.value.as_decimal().normalize().to_string(),
            size: level.size_decimal().normalize().to_string(),
        })
        .collect();

    levels.reverse();
    levels
}

#[cfg(test)]
mod tests {
    use nautilus_model::{data::OrderBookDelta, identifiers::InstrumentId};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn integrity_hash_group_expires_at_its_exact_deadline() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let timestamp = UnixNanos::from(17);

        let state = BookIntegrity {
            pending: Some(PendingHash {
                hash: "group".to_string(),
                deltas: OrderBookDeltas::new(
                    instrument_id,
                    vec![OrderBookDelta::clear(
                        instrument_id,
                        0,
                        timestamp,
                        timestamp,
                    )],
                ),
                deadline: Some(Instant::now() + Duration::from_secs(1)),
            }),
            latest_timestamp: timestamp,
            context: None,
            monitor_running: false,
            cancellation: CancellationToken::new(),
        };

        assert!(state.check_group(timestamp, "group").is_ok());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            state
                .check_group(timestamp, "group")
                .unwrap_err()
                .to_string(),
            "Previous book hash group could not be verified"
        );
    }
}
