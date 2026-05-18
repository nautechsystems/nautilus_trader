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

//! Parsers for converting Kraken L3 WebSocket messages to Nautilus domain models.

use std::cmp::Ordering;

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use chrono::{DateTime, Utc};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};

use super::{
    book_id::BookOrderIdHasher,
    messages::{KrakenL3EventType, KrakenL3OrderEvent, KrakenL3Snapshot, KrakenL3UpdateData},
};

fn datetime_to_nanos(value: DateTime<Utc>, field: &str) -> anyhow::Result<UnixNanos> {
    let nanos = value
        .timestamp_nanos_opt()
        .with_context(|| format!("Failed to convert {field}='{value}' to nanoseconds"))?;
    Ok(UnixNanos::from(u64::try_from(nanos).with_context(
        || format!("Timestamp predates Unix epoch: {field}='{value}'"),
    )?))
}

/// Cached state for an open L3 order, used to detect price changes on modify events.
#[derive(Debug, Clone)]
pub struct CachedL3Order {
    /// Last known limit price.
    pub price: f64,
    /// Original JSON decimal string for `price`, used verbatim in checksum computation.
    pub price_raw: String,
    /// Last known order quantity.
    pub size: f64,
    /// Original JSON decimal string for `size`, used verbatim in checksum computation.
    pub size_raw: String,
    /// Order side.
    pub side: OrderSide,
    /// Insertion sequence used to preserve FIFO queue order within a price level for checksum.
    pub seq: u64,
}

/// Parses a Kraken L3 snapshot into an `OrderBookDeltas` batch.
///
/// Emits a `Clear` delta followed by one `Add` delta per resting order.
///
/// # Errors
///
/// Returns an error if any price, quantity, or timestamp value cannot be parsed.
pub fn parse_l3_snapshot(
    snap: &KrakenL3Snapshot,
    instrument: &InstrumentAny,
    hasher: &BookOrderIdHasher,
    sequence: &mut u64,
    ts_init: UnixNanos,
    open_orders: &mut AHashMap<u64, CachedL3Order>,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let ts_event = datetime_to_nanos(snap.timestamp, "snap.timestamp")?;
    let snapshot_flags = RecordFlag::F_SNAPSHOT as u8;
    let total_orders = snap.bids.len() + snap.asks.len();

    let mut deltas = Vec::with_capacity(1 + total_orders);

    let mut clear = OrderBookDelta::clear(instrument_id, *sequence, ts_event, ts_init);
    *sequence += 1;

    if total_orders == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    let bid_iter = snap.bids.iter().map(|o| (o, OrderSide::Buy));
    let ask_iter = snap.asks.iter().map(|o| (o, OrderSide::Sell));

    let all_orders: Vec<_> = bid_iter.chain(ask_iter).collect();
    let last_idx = all_orders.len().saturating_sub(1);

    for (idx, (order, side)) in all_orders.into_iter().enumerate() {
        let price =
            Price::new_checked(order.limit_price.value, price_precision).with_context(|| {
                format!("Failed to construct Price with precision {price_precision}")
            })?;
        let size =
            Quantity::new_checked(order.order_qty.value, size_precision).with_context(|| {
                format!("Failed to construct Quantity with precision {size_precision}")
            })?;

        let order_id = hasher.hash(&order.order_id);
        let book_order = BookOrder::new(side, price, size, order_id);

        let mut flags = snapshot_flags;
        if idx == last_idx {
            flags |= RecordFlag::F_LAST as u8;
        }

        let insertion_seq = *sequence;
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            book_order,
            flags,
            *sequence,
            ts_event,
            ts_init,
        );
        *sequence += 1;

        open_orders.insert(
            order_id,
            CachedL3Order {
                price: order.limit_price.value,
                price_raw: order.limit_price.raw.clone(),
                size: order.order_qty.value,
                size_raw: order.order_qty.raw.clone(),
                side,
                seq: insertion_seq,
            },
        );

        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("Failed to construct OrderBookDeltas from L3 snapshot")
}

/// Parses a Kraken L3 incremental update into an optional `OrderBookDeltas` batch.
///
/// Returns the parsed `OrderBookDeltas` (or `None` when no deltas were produced) paired
/// with the update's `ts_event`. The `ts_event` is returned in both cases so callers can
/// run checksum verification and emit clear deltas even when the update produced no
/// outward changes (for example, a batch consisting only of `modify` events for orders
/// that were depth-pruned locally).
///
/// # Errors
///
/// Returns an error if any price, quantity, or timestamp value cannot be parsed.
pub fn parse_l3_update(
    update: &KrakenL3UpdateData,
    instrument: &InstrumentAny,
    hasher: &BookOrderIdHasher,
    sequence: &mut u64,
    ts_init: UnixNanos,
    open_orders: &mut AHashMap<u64, CachedL3Order>,
    depth: u32,
) -> anyhow::Result<(Option<OrderBookDeltas>, UnixNanos)> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let ts_event = datetime_to_nanos(update.timestamp, "update.timestamp")?;

    let bid_events = update.bids.iter().map(|e| (e, OrderSide::Buy));
    let ask_events = update.asks.iter().map(|e| (e, OrderSide::Sell));
    let all_events: Vec<(&KrakenL3OrderEvent, OrderSide)> = bid_events.chain(ask_events).collect();

    if all_events.is_empty() {
        return Ok((None, ts_event));
    }

    let mut deltas = Vec::new();

    for (event, side) in all_events {
        let order_id = hasher.hash(&event.order_id);

        match event.event {
            KrakenL3EventType::Add => {
                let price = Price::new_checked(event.limit_price.value, price_precision)
                    .with_context(|| {
                        format!("Failed to construct Price with precision {price_precision}")
                    })?;
                let size = Quantity::new_checked(event.order_qty.value, size_precision)
                    .with_context(|| {
                        format!("Failed to construct Quantity with precision {size_precision}")
                    })?;

                let book_order = BookOrder::new(side, price, size, order_id);
                let insertion_seq = *sequence;
                let delta = OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    book_order,
                    0,
                    *sequence,
                    ts_event,
                    ts_init,
                );
                *sequence += 1;

                open_orders.insert(
                    order_id,
                    CachedL3Order {
                        price: event.limit_price.value,
                        price_raw: event.limit_price.raw.clone(),
                        size: event.order_qty.value,
                        size_raw: event.order_qty.raw.clone(),
                        side,
                        seq: insertion_seq,
                    },
                );

                deltas.push(delta);
            }
            KrakenL3EventType::Modify => {
                if let Some(cached) = open_orders.get_mut(&order_id) {
                    let new_price = Price::new_checked(event.limit_price.value, price_precision)
                        .with_context(|| {
                            format!(
                                "Failed to construct new Price with precision {price_precision}"
                            )
                        })?;
                    let new_size = Quantity::new_checked(event.order_qty.value, size_precision)
                        .with_context(|| {
                            format!(
                                "Failed to construct new Quantity with precision {size_precision}"
                            )
                        })?;

                    // Compare the raw wire decimal so a sub-tick discrepancy still
                    // takes the price-change branch rather than rounding to the
                    // same `Price` and leaving the cached FIFO `seq` in place.
                    if cached.price_raw == event.limit_price.raw {
                        let book_order = BookOrder::new(cached.side, new_price, new_size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            BookAction::Update,
                            book_order,
                            0,
                            *sequence,
                            ts_event,
                            ts_init,
                        );
                        *sequence += 1;

                        cached.price = event.limit_price.value;
                        cached.price_raw = event.limit_price.raw.clone();
                        cached.size = event.order_qty.value;
                        cached.size_raw = event.order_qty.raw.clone();

                        deltas.push(delta);
                    } else {
                        let cached_price = Price::new_checked(cached.price, price_precision)
                            .with_context(|| {
                                format!(
                                    "Failed to construct cached Price with precision \
                                         {price_precision}"
                                )
                            })?;
                        let old_size = Quantity::new_checked(cached.size, size_precision)
                            .with_context(|| {
                                format!(
                                    "Failed to construct old Quantity with precision \
                                         {size_precision}"
                                )
                            })?;
                        let del_order =
                            BookOrder::new(cached.side, cached_price, old_size, order_id);
                        let del_delta = OrderBookDelta::new_checked(
                            instrument_id,
                            BookAction::Delete,
                            del_order,
                            0,
                            *sequence,
                            ts_event,
                            ts_init,
                        )
                        .context(
                            "Failed to construct Delete OrderBookDelta for price-change modify",
                        )?;
                        *sequence += 1;
                        deltas.push(del_delta);

                        let add_order = BookOrder::new(cached.side, new_price, new_size, order_id);
                        let new_seq = *sequence;
                        let add_delta = OrderBookDelta::new(
                            instrument_id,
                            BookAction::Add,
                            add_order,
                            0,
                            *sequence,
                            ts_event,
                            ts_init,
                        );
                        *sequence += 1;

                        cached.price = event.limit_price.value;
                        cached.price_raw = event.limit_price.raw.clone();
                        cached.size = event.order_qty.value;
                        cached.size_raw = event.order_qty.raw.clone();
                        cached.seq = new_seq;

                        deltas.push(add_delta);
                    }
                } else {
                    log::warn!(
                        "Unknown order_id on Modify event: {} — skipping",
                        event.order_id
                    );
                }
            }
            KrakenL3EventType::Delete => {
                if let Some(cached) = open_orders.remove(&order_id) {
                    let price =
                        Price::new_checked(cached.price, price_precision).with_context(|| {
                            format!("Failed to construct Price with precision {price_precision}")
                        })?;
                    let size =
                        Quantity::new_checked(cached.size, size_precision).with_context(|| {
                            format!("Failed to construct Quantity with precision {size_precision}")
                        })?;
                    let book_order = BookOrder::new(cached.side, price, size, order_id);
                    let delta = OrderBookDelta::new_checked(
                        instrument_id,
                        BookAction::Delete,
                        book_order,
                        0,
                        *sequence,
                        ts_event,
                        ts_init,
                    )
                    .context("Failed to construct Delete OrderBookDelta")?;
                    *sequence += 1;
                    deltas.push(delta);
                } else {
                    log::warn!(
                        "Unknown order_id on Delete event: {} — ignoring",
                        event.order_id
                    );
                }
            }
        }
    }

    if deltas.is_empty() {
        return Ok((None, ts_event));
    }

    append_depth_prune_deltas(
        instrument_id,
        price_precision,
        size_precision,
        depth,
        sequence,
        ts_event,
        ts_init,
        open_orders,
        &mut deltas,
    )?;

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    let book_deltas = OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("Failed to construct OrderBookDeltas from L3 update")?;
    Ok((Some(book_deltas), ts_event))
}

#[expect(clippy::too_many_arguments)]
fn append_depth_prune_deltas(
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    depth: u32,
    sequence: &mut u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    open_orders: &mut AHashMap<u64, CachedL3Order>,
    deltas: &mut Vec<OrderBookDelta>,
) -> anyhow::Result<()> {
    if depth == 0 || open_orders.is_empty() {
        return Ok(());
    }

    let mut ask_levels: AHashSet<u64> = AHashSet::new();
    let mut bid_levels: AHashSet<u64> = AHashSet::new();

    for order in open_orders.values() {
        match order.side {
            OrderSide::Sell => {
                ask_levels.insert(order.price.to_bits());
            }
            OrderSide::Buy => {
                bid_levels.insert(order.price.to_bits());
            }
            OrderSide::NoOrderSide => {}
        }
    }

    let depth_usize = depth as usize;
    if ask_levels.len() <= depth_usize && bid_levels.len() <= depth_usize {
        return Ok(());
    }

    let ask_keep = if ask_levels.len() > depth_usize {
        retained_price_levels(open_orders, OrderSide::Sell, depth)
    } else {
        ask_levels
    };
    let bid_keep = if bid_levels.len() > depth_usize {
        retained_price_levels(open_orders, OrderSide::Buy, depth)
    } else {
        bid_levels
    };

    let mut pruned: Vec<(u64, CachedL3Order)> = open_orders
        .iter()
        .filter(|(_, order)| match order.side {
            OrderSide::Sell => !ask_keep.contains(&order.price.to_bits()),
            OrderSide::Buy => !bid_keep.contains(&order.price.to_bits()),
            OrderSide::NoOrderSide => true,
        })
        .map(|(order_id, order)| (*order_id, order.clone()))
        .collect();

    pruned.sort_by(|(_, a), (_, b)| compare_l3_orders(a, b));

    for (order_id, cached) in pruned {
        open_orders.remove(&order_id);

        let price = Price::new_checked(cached.price, price_precision).with_context(|| {
            format!("Failed to construct Price with precision {price_precision}")
        })?;
        let size = Quantity::new_checked(cached.size, size_precision).with_context(|| {
            format!("Failed to construct Quantity with precision {size_precision}")
        })?;
        let book_order = BookOrder::new(cached.side, price, size, order_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Delete,
            book_order,
            0,
            *sequence,
            ts_event,
            ts_init,
        )
        .context("Failed to construct Delete OrderBookDelta for L3 depth pruning")?;
        *sequence += 1;
        deltas.push(delta);
    }

    Ok(())
}

fn retained_price_levels(
    open_orders: &AHashMap<u64, CachedL3Order>,
    side: OrderSide,
    depth: u32,
) -> AHashSet<u64> {
    let mut levels: Vec<(f64, u64)> = open_orders
        .values()
        .filter(|order| order.side == side)
        .map(|order| (order.price, order.price.to_bits()))
        .collect();

    match side {
        OrderSide::Sell => levels.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(Ordering::Equal)
                .then(a.1.cmp(&b.1))
        }),
        OrderSide::Buy => levels.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then(a.1.cmp(&b.1))
        }),
        OrderSide::NoOrderSide => {}
    }

    levels.dedup_by(|a, b| a.1 == b.1);
    levels
        .into_iter()
        .take(depth as usize)
        .map(|(_, price_bits)| price_bits)
        .collect()
}

fn compare_l3_orders(a: &CachedL3Order, b: &CachedL3Order) -> Ordering {
    let side_order = side_rank(a.side).cmp(&side_rank(b.side));
    if side_order != Ordering::Equal {
        return side_order;
    }

    let price_order = match a.side {
        OrderSide::Sell => a.price.partial_cmp(&b.price).unwrap_or(Ordering::Equal),
        OrderSide::Buy => b.price.partial_cmp(&a.price).unwrap_or(Ordering::Equal),
        OrderSide::NoOrderSide => Ordering::Equal,
    };

    price_order.then(a.seq.cmp(&b.seq))
}

fn side_rank(side: OrderSide) -> u8 {
    match side {
        OrderSide::Sell => 0,
        OrderSide::Buy => 1,
        OrderSide::NoOrderSide => 2,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        enums::{BookAction, RecordFlag},
        identifiers::{InstrumentId, Symbol},
        instruments::{InstrumentAny, currency_pair::CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

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

    fn load_snapshot() -> KrakenL3Snapshot {
        let json = include_str!("../../../../test_data/ws_l3_snapshot.json");
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        serde_json::from_value(v["data"][0].clone()).unwrap()
    }

    fn load_update(filename: &str) -> KrakenL3UpdateData {
        let path = format!("{}/test_data/{filename}", env!("CARGO_MANIFEST_DIR"));
        let json = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        serde_json::from_value(v["data"][0].clone()).unwrap()
    }

    fn populated_open_orders() -> AHashMap<u64, CachedL3Order> {
        let snap = load_snapshot();
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = AHashMap::new();
        parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
        )
        .unwrap();
        open_orders
    }

    #[rstest]
    fn test_parse_l3_snapshot_delta_count() {
        let snap = load_snapshot();
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = AHashMap::new();

        let result = parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
        )
        .unwrap();

        assert_eq!(result.deltas.len(), 6); // 1 Clear + 3 bids + 2 asks
        assert_eq!(result.deltas[0].action, BookAction::Clear);
        assert_eq!(open_orders.len(), 5);
    }

    #[rstest]
    fn test_parse_l3_snapshot_flags() {
        let snap = load_snapshot();
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = AHashMap::new();

        let result = parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
        )
        .unwrap();

        for delta in &result.deltas {
            assert!(
                RecordFlag::F_SNAPSHOT.matches(delta.flags),
                "expected F_SNAPSHOT on all deltas"
            );
        }

        let last = result.deltas.last().unwrap();
        assert!(
            RecordFlag::F_LAST.matches(last.flags),
            "expected F_LAST on last delta"
        );

        for delta in result.deltas.iter().take(result.deltas.len() - 1) {
            assert!(
                !RecordFlag::F_LAST.matches(delta.flags),
                "expected no F_LAST on non-last deltas"
            );
        }
    }

    #[rstest]
    fn test_parse_l3_update_add() {
        let update = load_update("ws_l3_update_add.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 1);
        assert_eq!(result.deltas[0].action, BookAction::Add);
        assert_eq!(open_orders.len(), 6);
    }

    #[rstest]
    fn test_parse_l3_update_prunes_orders_beyond_depth() {
        let update = load_update("ws_l3_update_add.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();
        let pruned_order_id = hasher.hash("order-bid-3");

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            2,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 2);
        assert_eq!(result.deltas[0].action, BookAction::Add);
        assert_eq!(result.deltas[1].action, BookAction::Delete);
        assert!(RecordFlag::F_LAST.matches(result.deltas[1].flags));
        assert!(!open_orders.contains_key(&pruned_order_id));
        assert_eq!(open_orders.len(), 5);
    }

    #[rstest]
    fn test_parse_l3_update_modify_same_price_produces_update() {
        let update = load_update("ws_l3_update_modify_qty.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 1);
        assert_eq!(result.deltas[0].action, BookAction::Update);
    }

    #[rstest]
    fn test_parse_l3_update_modify_same_numeric_price_differing_raw_triggers_delete_add() {
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();
        let order_id = hasher.hash("order-ask-1");
        let cached_raw = open_orders
            .get(&order_id)
            .expect("seeded order-ask-1 in snapshot")
            .price_raw
            .clone();
        assert_eq!(cached_raw, "42001.0");

        // Deserialize the data payload directly so the wire-format raw decimal
        // is preserved; routing through `serde_json::Value` would collapse
        // `42001.00` back to `42001.0` and defeat the test.
        let update_data_json = r#"{
            "symbol": "BTC/USD",
            "timestamp": "2024-01-15T12:00:02.000000Z",
            "checksum": 0,
            "bids": [],
            "asks": [
                {
                    "event": "modify",
                    "order_id": "order-ask-1",
                    "limit_price": 42001.00,
                    "order_qty": 0.5,
                    "timestamp": "2024-01-15T12:00:02.000000Z"
                }
            ]
        }"#;
        let update: KrakenL3UpdateData = serde_json::from_str(update_data_json).unwrap();
        assert_eq!(update.asks[0].limit_price.raw, "42001.00");
        assert!(
            (update.asks[0].limit_price.value - 42001.0).abs() < f64::EPSILON,
            "numeric value should match cached price within f64 precision"
        );

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 2);
        assert_eq!(result.deltas[0].action, BookAction::Delete);
        assert_eq!(result.deltas[1].action, BookAction::Add);

        let cached_after = open_orders
            .get(&order_id)
            .expect("order should remain after modify");
        assert_eq!(cached_after.price_raw, "42001.00");
    }

    #[rstest]
    fn test_parse_l3_update_modify_price_change_produces_delete_add() {
        let update = load_update("ws_l3_update_modify_price.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 2);
        assert_eq!(result.deltas[0].action, BookAction::Delete);
        assert_eq!(result.deltas[1].action, BookAction::Add);
    }

    #[rstest]
    fn test_parse_l3_update_delete() {
        let update = load_update("ws_l3_update_delete.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 1);
        assert_eq!(result.deltas[0].action, BookAction::Delete);
        assert_eq!(open_orders.len(), 4);
    }

    #[rstest]
    fn test_parse_l3_update_empty_returns_none() {
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut open_orders = AHashMap::new();

        let empty_update = KrakenL3UpdateData {
            symbol: "BTC/USD".to_string(),
            bids: vec![],
            asks: vec![],
            checksum: 0,
            timestamp: chrono::Utc::now(),
        };

        let (deltas, _ts_event) = parse_l3_update(
            &empty_update,
            &instrument,
            &hasher,
            &mut sequence,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();

        assert!(deltas.is_none());
    }
}
