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

//! Shared protocol state machine for the Kraken Spot `level3` channel.
//!
//! The Rust `KrakenSpotDataClient` and the pyo3 `KrakenSpotWebSocketClient`
//! stream loop drive their per-symbol state through [`process_l3_message`],
//! keeping the snapshot parsing, incremental update parsing, checksum
//! validation, and resync logic in a single implementation. Each caller
//! supplies an [`L3Sink`] that decides how to deliver the produced
//! `OrderBookDeltas` to its downstream consumer.

use std::sync::{Arc, Mutex};

use ahash::AHashMap;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_model::{
    data::{OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::RecordFlag,
    identifiers::{InstrumentId, Symbol},
    instruments::{Instrument, InstrumentAny},
};

use super::{
    BookOrderIdHasher, KrakenL3WsMessage,
    checksum::{build_checksum_string, compute_checksum},
    parse::{CachedL3Order, parse_l3_snapshot, parse_l3_update},
};
use crate::common::consts::KRAKEN_VENUE;

/// Per-symbol L3 state tracked between WebSocket messages.
#[derive(Debug)]
pub struct L3State {
    /// Monotonically increasing sequence counter applied to outbound deltas.
    pub sequence: u64,
    /// Subscription depth in price levels (one of `10`, `100`, `1000`).
    pub depth: u32,
    /// Whether the next snapshot has not yet been received after a (re)connect.
    pub awaiting_snapshot: bool,
    /// Resting orders keyed by hashed venue order ID.
    pub open_orders: AHashMap<u64, CachedL3Order>,
}

/// Request emitted when local state diverges from Kraken's reported checksum.
///
/// The caller should unsubscribe and resubscribe the named symbol to obtain a
/// fresh snapshot.
#[derive(Debug)]
pub struct L3ResyncRequest {
    /// Kraken symbol that requires resync.
    pub symbol: String,
    /// Depth to resubscribe with.
    pub depth: u32,
    /// Short description of why the resync was requested.
    pub reason: &'static str,
}

/// Output sink for `OrderBookDeltas` produced by the L3 state machine.
pub trait L3Sink {
    /// Forwards a batch of L3 deltas to the consumer.
    fn emit_deltas(&mut self, deltas: OrderBookDeltas_API);
}

/// Returns the depth registered for `symbol`, defaulting to `1000`.
pub fn subscription_depth(depths: &Arc<Mutex<AHashMap<String, u32>>>, symbol: &str) -> u32 {
    depths
        .lock()
        .map_or(1000, |depths| depths.get(symbol).copied().unwrap_or(1000))
}

fn lookup_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    symbol: &str,
) -> Option<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(symbol), *KRAKEN_VENUE);
    instruments.load().get(&instrument_id).cloned()
}

/// Emits a `Clear` delta (with `F_LAST`) after detecting a checksum mismatch.
pub fn emit_l3_clear<S: L3Sink>(
    sink: &mut S,
    instrument_id: InstrumentId,
    sequence: &mut u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    reason: &str,
) {
    let mut clear = OrderBookDelta::clear(instrument_id, *sequence, ts_event, ts_init);
    *sequence += 1;
    clear.flags |= RecordFlag::F_LAST as u8;

    match OrderBookDeltas::new_checked(instrument_id, vec![clear]) {
        Ok(clear_deltas) => sink.emit_deltas(OrderBookDeltas_API::new(clear_deltas)),
        Err(e) => log::error!("Failed to construct L3 clear after {reason}: {e}"),
    }
}

/// Drives the L3 protocol state machine for one inbound message.
///
/// Returns `Some(L3ResyncRequest)` when a checksum mismatch requires the
/// caller to resync the affected symbol via unsubscribe + subscribe.
#[expect(clippy::too_many_arguments)]
pub fn process_l3_message<S: L3Sink>(
    msg: KrakenL3WsMessage,
    sink: &mut S,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    depths: &Arc<Mutex<AHashMap<String, u32>>>,
    states: &mut AHashMap<String, L3State>,
    hasher: &BookOrderIdHasher,
    validate_checksum: bool,
    ts_init: UnixNanos,
) -> Option<L3ResyncRequest> {
    match msg {
        KrakenL3WsMessage::Snapshot(snap) => {
            let Some(instrument) = lookup_instrument(instruments, &snap.symbol) else {
                log::warn!("L3 snapshot: no instrument for symbol={}", snap.symbol);
                return None;
            };

            let state = states
                .entry(snap.symbol.clone())
                .or_insert_with(|| L3State {
                    sequence: 0,
                    depth: subscription_depth(depths, &snap.symbol),
                    awaiting_snapshot: false,
                    open_orders: AHashMap::new(),
                });
            state.depth = subscription_depth(depths, &snap.symbol);
            state.open_orders.clear();

            match parse_l3_snapshot(
                &snap,
                &instrument,
                hasher,
                &mut state.sequence,
                ts_init,
                &mut state.open_orders,
            ) {
                Ok(deltas) => {
                    if validate_checksum && snap.checksum != 0 {
                        let local = compute_checksum(&state.open_orders);
                        if local == snap.checksum {
                            log::debug!(
                                "L3 snapshot checksum OK: symbol={}, checksum={local}, orders={}",
                                snap.symbol,
                                state.open_orders.len(),
                            );
                        } else {
                            let s = build_checksum_string(&state.open_orders);
                            log::warn!(
                                "L3 snapshot checksum mismatch: symbol={}, local={local}, \
                                 remote={}, orders={}, string_len={}, prefix={:?}, clearing state",
                                snap.symbol,
                                snap.checksum,
                                state.open_orders.len(),
                                s.len(),
                                &s[..s.len().min(200)],
                            );
                            state.open_orders.clear();
                            state.awaiting_snapshot = true;
                            emit_l3_clear(
                                sink,
                                instrument.id(),
                                &mut state.sequence,
                                deltas.ts_event,
                                ts_init,
                                "snapshot checksum mismatch",
                            );
                            return Some(L3ResyncRequest {
                                symbol: snap.symbol,
                                depth: state.depth,
                                reason: "snapshot checksum mismatch",
                            });
                        }
                    }
                    state.awaiting_snapshot = false;
                    sink.emit_deltas(OrderBookDeltas_API::new(deltas));
                }
                Err(e) => {
                    log::error!(
                        "Failed to parse L3 snapshot for {}: {e}, clearing state and resyncing",
                        snap.symbol,
                    );
                    state.open_orders.clear();
                    state.awaiting_snapshot = true;
                    emit_l3_clear(
                        sink,
                        instrument.id(),
                        &mut state.sequence,
                        ts_init,
                        ts_init,
                        "snapshot parse error",
                    );
                    return Some(L3ResyncRequest {
                        symbol: snap.symbol,
                        depth: state.depth,
                        reason: "snapshot parse error",
                    });
                }
            }
        }
        KrakenL3WsMessage::Update(update) => {
            let Some(instrument) = lookup_instrument(instruments, &update.symbol) else {
                log::warn!("L3 update: no instrument for symbol={}", update.symbol);
                return None;
            };

            let state = states
                .entry(update.symbol.clone())
                .or_insert_with(|| L3State {
                    sequence: 0,
                    depth: subscription_depth(depths, &update.symbol),
                    awaiting_snapshot: true,
                    open_orders: AHashMap::new(),
                });
            state.depth = subscription_depth(depths, &update.symbol);

            if state.awaiting_snapshot {
                log::debug!(
                    "Ignoring L3 update while awaiting snapshot: symbol={}",
                    update.symbol
                );
                return None;
            }

            match parse_l3_update(
                &update,
                &instrument,
                hasher,
                &mut state.sequence,
                ts_init,
                &mut state.open_orders,
                state.depth,
            ) {
                Ok((maybe_deltas, ts_event)) => {
                    if validate_checksum {
                        let local = compute_checksum(&state.open_orders);
                        if local != update.checksum {
                            let s = build_checksum_string(&state.open_orders);
                            log::warn!(
                                "L3 checksum mismatch: symbol={}, local={local}, remote={}, \
                                 orders={}, string_len={}, prefix={:?}, clearing state",
                                update.symbol,
                                update.checksum,
                                state.open_orders.len(),
                                s.len(),
                                &s[..s.len().min(200)],
                            );
                            state.open_orders.clear();
                            state.awaiting_snapshot = true;
                            emit_l3_clear(
                                sink,
                                instrument.id(),
                                &mut state.sequence,
                                ts_event,
                                ts_init,
                                "update checksum mismatch",
                            );
                            return Some(L3ResyncRequest {
                                symbol: update.symbol,
                                depth: state.depth,
                                reason: "update checksum mismatch",
                            });
                        }
                    }

                    if let Some(deltas) = maybe_deltas {
                        sink.emit_deltas(OrderBookDeltas_API::new(deltas));
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to parse L3 update for {}: {e}, clearing state and resyncing",
                        update.symbol,
                    );
                    state.open_orders.clear();
                    state.awaiting_snapshot = true;
                    emit_l3_clear(
                        sink,
                        instrument.id(),
                        &mut state.sequence,
                        ts_init,
                        ts_init,
                        "update parse error",
                    );
                    return Some(L3ResyncRequest {
                        symbol: update.symbol,
                        depth: state.depth,
                        reason: "update parse error",
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::BookAction,
        instruments::currency_pair::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::websocket::spot_v2::level_3::messages::{KrakenL3Snapshot, KrakenL3UpdateData};

    fn make_instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            InstrumentId::from("BTC/USD.KRAKEN"),
            Symbol::from("BTC/USD"),
            Currency::BTC(),
            Currency::USD(),
            1,
            8,
            Price::from("0.1"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    struct CollectingSink {
        emitted: Vec<OrderBookDeltas_API>,
    }

    impl L3Sink for CollectingSink {
        fn emit_deltas(&mut self, deltas: OrderBookDeltas_API) {
            self.emitted.push(deltas);
        }
    }

    #[rstest]
    fn test_process_l3_message_snapshot_checksum_mismatch_requests_resync() {
        let instruments = Arc::new(AtomicMap::new());
        let instrument = make_instrument();
        instruments.insert(instrument.id(), instrument);

        let depths = Arc::new(Mutex::new(AHashMap::new()));
        depths
            .lock()
            .expect("L3 depth map mutex poisoned")
            .insert("BTC/USD".to_string(), 1000);

        let snapshot: KrakenL3Snapshot = serde_json::from_str(
            r#"{
                "symbol": "BTC/USD",
                "bids": [{
                    "order_id": "order-bid-1",
                    "limit_price": 4199.0,
                    "order_qty": 3.00000000,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "asks": [{
                    "order_id": "order-ask-1",
                    "limit_price": 4200.0,
                    "order_qty": 0.01000000,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "checksum": 1,
                "timestamp": "2024-01-01T00:00:00Z"
            }"#,
        )
        .unwrap();

        let mut sink = CollectingSink {
            emitted: Vec::new(),
        };
        let mut states = AHashMap::new();
        let hasher = BookOrderIdHasher::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = process_l3_message(
            KrakenL3WsMessage::Snapshot(snapshot),
            &mut sink,
            &instruments,
            &depths,
            &mut states,
            &hasher,
            true,
            ts_init,
        )
        .expect("expected resync request");

        assert_eq!(request.symbol, "BTC/USD");
        assert_eq!(request.depth, 1000);
        assert_eq!(request.reason, "snapshot checksum mismatch");

        assert_eq!(sink.emitted.len(), 1);
        let clear = &sink.emitted[0];
        assert_eq!(clear.deltas.len(), 1);
        assert_eq!(clear.deltas[0].action, BookAction::Clear);
        assert!(states["BTC/USD"].awaiting_snapshot);
        assert!(states["BTC/USD"].open_orders.is_empty());
    }

    #[rstest]
    fn test_process_l3_message_update_parse_error_clears_state_and_resyncs() {
        let instruments = Arc::new(AtomicMap::new());
        let instrument = make_instrument();
        instruments.insert(instrument.id(), instrument);

        let depths = Arc::new(Mutex::new(AHashMap::new()));
        depths
            .lock()
            .expect("L3 depth map mutex poisoned")
            .insert("BTC/USD".to_string(), 1000);

        let update: KrakenL3UpdateData = serde_json::from_str(
            r#"{
                "symbol": "BTC/USD",
                "bids": [{
                    "event": "add",
                    "order_id": "bad",
                    "limit_price": 1e20,
                    "order_qty": 0.1,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "asks": [],
                "checksum": 0,
                "timestamp": "2024-01-01T00:00:01Z"
            }"#,
        )
        .unwrap();

        let mut sink = CollectingSink {
            emitted: Vec::new(),
        };
        let mut states = AHashMap::new();
        states.insert(
            "BTC/USD".to_string(),
            L3State {
                sequence: 42,
                depth: 1000,
                awaiting_snapshot: false,
                open_orders: AHashMap::new(),
            },
        );
        let hasher = BookOrderIdHasher::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = process_l3_message(
            KrakenL3WsMessage::Update(update),
            &mut sink,
            &instruments,
            &depths,
            &mut states,
            &hasher,
            true,
            ts_init,
        )
        .expect("expected resync request");

        assert_eq!(request.symbol, "BTC/USD");
        assert_eq!(request.depth, 1000);
        assert_eq!(request.reason, "update parse error");

        assert_eq!(sink.emitted.len(), 1);
        let clear = &sink.emitted[0];
        assert_eq!(clear.deltas.len(), 1);
        assert_eq!(clear.deltas[0].action, BookAction::Clear);
        assert!(states["BTC/USD"].awaiting_snapshot);
        assert!(states["BTC/USD"].open_orders.is_empty());
    }
}
