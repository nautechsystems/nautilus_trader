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
use jiff::Timestamp;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::{
    book_id::BookOrderIdHasher,
    messages::{KrakenL3EventType, KrakenL3OrderEvent, KrakenL3Snapshot, KrakenL3UpdateData},
};

fn datetime_to_nanos(value: Timestamp, field: &str) -> anyhow::Result<UnixNanos> {
    Ok(UnixNanos::from(
        u64::try_from(value.as_nanosecond())
            .with_context(|| format!("Timestamp predates Unix epoch: {field}='{value}'"))?,
    ))
}

/// Cached state for an open L3 order, used to detect price changes on modify events.
#[derive(Debug, Clone)]
pub(crate) struct CachedL3Order {
    /// Last known limit price.
    pub price: Decimal,
    /// Original JSON decimal string for `price`, used verbatim in checksum computation.
    pub price_raw: String,
    /// Last known order quantity.
    pub size: Decimal,
    /// Original JSON decimal string for `size`, used verbatim in checksum computation.
    pub size_raw: String,
    /// Order side.
    pub side: OrderSide,
    /// Kraken's order timestamp, which sets queue priority within a price level.
    pub timestamp: UnixNanos,
    /// Arrival order, which breaks timestamp ties within a price level.
    ///
    /// Assigned when Kraken places the order (snapshot, add, or requeuing modify) and kept when
    /// the order is re-added downstream, so it is independent of the outbound delta sequence.
    pub seq: u64,
}

/// Parses a Kraken L3 snapshot into an `OrderBookDeltas` batch.
///
/// Emits a `Clear` delta followed by one `Add` delta per resting order, with each price level's
/// orders in queue order (timestamp, then snapshot position).
///
/// # Errors
///
/// Returns an error if any price, quantity, or timestamp value cannot be parsed.
pub(crate) fn parse_l3_snapshot(
    snap: &KrakenL3Snapshot,
    instrument: &InstrumentAny,
    hasher: &BookOrderIdHasher,
    sequence: &mut u64,
    arrival: &mut u64,
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

    // The downstream book queues each `Add` at the back of its level, so emit every level in
    // queue order; the stable sort keeps the snapshot's own order for equal timestamps
    let mut all_orders: Vec<_> = bid_iter.chain(ask_iter).collect();
    all_orders.sort_by(|(a, a_side), (b, b_side)| {
        // Bids first, as the snapshot lists them
        let side_order = side_rank(*b_side).cmp(&side_rank(*a_side));
        if side_order != Ordering::Equal {
            return side_order;
        }

        let price_order = match a_side {
            OrderSide::Sell => a.limit_price.value.cmp(&b.limit_price.value),
            OrderSide::Buy => b.limit_price.value.cmp(&a.limit_price.value),
        };
        price_order.then(a.timestamp.cmp(&b.timestamp))
    });
    let last_idx = all_orders.len().saturating_sub(1);

    for (idx, (order, side)) in all_orders.into_iter().enumerate() {
        let price = Price::from_decimal_dp(order.limit_price.value, price_precision).with_context(
            || format!("Failed to construct Price with precision {price_precision}"),
        )?;
        let size = Quantity::from_decimal_dp(order.order_qty.value, size_precision).with_context(
            || format!("Failed to construct Quantity with precision {size_precision}"),
        )?;

        let timestamp = datetime_to_nanos(order.timestamp, "order.timestamp")?;
        let order_id = hasher.hash(&order.order_id);
        let book_order = BookOrder::new(side, price, size, order_id);

        let mut flags = snapshot_flags;
        if idx == last_idx {
            flags |= RecordFlag::F_LAST as u8;
        }

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
                timestamp,
                seq: next_arrival(arrival),
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
/// Kraken queues each price level by order timestamp, so an order can join ahead of orders
/// already resting there. The emitted batch keeps the downstream queue in that order (see
/// [`emit_queued_add`]), and sequences its deltas in emission order.
///
/// # Errors
///
/// Returns an error if any price, quantity, or timestamp value cannot be parsed.
#[expect(clippy::too_many_arguments)]
pub(crate) fn parse_l3_update(
    update: &KrakenL3UpdateData,
    instrument: &InstrumentAny,
    hasher: &BookOrderIdHasher,
    sequence: &mut u64,
    arrival: &mut u64,
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

    let mut batch = L3DeltaBatch::default();

    for (event, side) in all_events {
        let order_id = hasher.hash(&event.order_id);

        match event.event {
            KrakenL3EventType::Add => {
                let price = Price::from_decimal_dp(event.limit_price.value, price_precision)
                    .with_context(|| {
                        format!("Failed to construct Price with precision {price_precision}")
                    })?;
                let size = Quantity::from_decimal_dp(event.order_qty.value, size_precision)
                    .with_context(|| {
                        format!("Failed to construct Quantity with precision {size_precision}")
                    })?;
                let timestamp = datetime_to_nanos(event.timestamp, "event.timestamp")?;

                let book_order = BookOrder::new(side, price, size, order_id);
                let delta = OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    book_order,
                    0,
                    0,
                    ts_event,
                    ts_init,
                );

                open_orders.insert(
                    order_id,
                    CachedL3Order {
                        price: event.limit_price.value,
                        price_raw: event.limit_price.raw.clone(),
                        size: event.order_qty.value,
                        size_raw: event.order_qty.raw.clone(),
                        side,
                        timestamp,
                        seq: next_arrival(arrival),
                    },
                );

                emit_queued_add(
                    &mut batch,
                    delta,
                    open_orders,
                    price_precision,
                    size_precision,
                )?;
            }
            KrakenL3EventType::Modify => {
                if let Some(cached) = open_orders.get_mut(&order_id) {
                    let new_price =
                        Price::from_decimal_dp(event.limit_price.value, price_precision)
                            .with_context(|| {
                                format!(
                                    "Failed to construct new Price with precision {price_precision}"
                                )
                            })?;
                    let new_size = Quantity::from_decimal_dp(event.order_qty.value, size_precision)
                        .with_context(|| {
                            format!(
                                "Failed to construct new Quantity with precision {size_precision}"
                            )
                        })?;

                    // Compare the raw wire decimal so a sub-tick discrepancy still
                    // takes the price-change branch rather than rounding to the
                    // same `Price` and leaving the cached queue position in place.
                    // Kraken keeps queue priority only when the size decreases. Any
                    // other modify (a larger or unchanged size) also takes the
                    // delete/add branch and is requeued by its new timestamp.
                    if cached.price_raw == event.limit_price.raw
                        && event.order_qty.value < cached.size
                    {
                        let book_order = BookOrder::new(cached.side, new_price, new_size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            BookAction::Update,
                            book_order,
                            0,
                            0,
                            ts_event,
                            ts_init,
                        );

                        cached.price = event.limit_price.value;
                        cached.price_raw = event.limit_price.raw.clone();
                        cached.size = event.order_qty.value;
                        cached.size_raw = event.order_qty.raw.clone();

                        batch.push(delta);
                    } else {
                        let timestamp = datetime_to_nanos(event.timestamp, "event.timestamp")?;
                        let cached_price = Price::from_decimal_dp(cached.price, price_precision)
                            .with_context(|| {
                                format!(
                                    "Failed to construct cached Price with precision \
                                         {price_precision}"
                                )
                            })?;
                        let old_size = Quantity::from_decimal_dp(cached.size, size_precision)
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
                            0,
                            ts_event,
                            ts_init,
                        )
                        .context("Failed to construct Delete OrderBookDelta for requeued modify")?;
                        batch.push(del_delta);

                        let add_order = BookOrder::new(cached.side, new_price, new_size, order_id);
                        let add_delta = OrderBookDelta::new(
                            instrument_id,
                            BookAction::Add,
                            add_order,
                            0,
                            0,
                            ts_event,
                            ts_init,
                        );

                        cached.price = event.limit_price.value;
                        cached.price_raw = event.limit_price.raw.clone();
                        cached.size = event.order_qty.value;
                        cached.size_raw = event.order_qty.raw.clone();
                        cached.timestamp = timestamp;
                        cached.seq = next_arrival(arrival);

                        emit_queued_add(
                            &mut batch,
                            add_delta,
                            open_orders,
                            price_precision,
                            size_precision,
                        )?;
                    }
                } else {
                    log::debug!(
                        "Unknown order_id on Modify event: {} - skipping",
                        event.order_id
                    );
                }
            }
            KrakenL3EventType::Delete => {
                if let Some(cached) = open_orders.remove(&order_id) {
                    let price = Price::from_decimal_dp(cached.price, price_precision)
                        .with_context(|| {
                            format!("Failed to construct Price with precision {price_precision}")
                        })?;
                    let size = Quantity::from_decimal_dp(cached.size, size_precision)
                        .with_context(|| {
                            format!("Failed to construct Quantity with precision {size_precision}")
                        })?;
                    let book_order = BookOrder::new(cached.side, price, size, order_id);
                    let delta = OrderBookDelta::new_checked(
                        instrument_id,
                        BookAction::Delete,
                        book_order,
                        0,
                        0,
                        ts_event,
                        ts_init,
                    )
                    .context("Failed to construct Delete OrderBookDelta")?;
                    batch.push(delta);
                } else {
                    log::debug!(
                        "Unknown order_id on Delete event: {} - ignoring",
                        event.order_id
                    );
                }
            }
        }
    }

    if batch.deltas.is_empty() {
        return Ok((None, ts_event));
    }

    append_depth_prune_deltas(
        instrument_id,
        price_precision,
        size_precision,
        depth,
        ts_event,
        ts_init,
        open_orders,
        &mut batch,
    )?;

    let mut deltas = batch.deltas;
    for delta in &mut deltas {
        delta.sequence = *sequence;
        *sequence += 1;
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    let book_deltas = OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("Failed to construct OrderBookDeltas from L3 update")?;
    Ok((Some(book_deltas), ts_event))
}

/// Deltas for one L3 update, in emission order, before sequence numbers are assigned.
#[derive(Debug, Default)]
struct L3DeltaBatch {
    deltas: Vec<OrderBookDelta>,
    /// Index and action of the latest delta for each order in `deltas`.
    latest: AHashMap<u64, (usize, BookAction)>,
}

impl L3DeltaBatch {
    fn push(&mut self, delta: OrderBookDelta) {
        self.latest
            .insert(delta.order.order_id, (self.deltas.len(), delta.action));
        self.deltas.push(delta);
    }

    fn insert(&mut self, index: usize, delta: OrderBookDelta) {
        for (latest_index, _) in self.latest.values_mut() {
            if *latest_index >= index {
                *latest_index += 1;
            }
        }
        self.latest
            .insert(delta.order.order_id, (index, delta.action));
        self.deltas.insert(index, delta);
    }
}

/// Emits `add` for an order already placed in `open_orders`, so that the downstream queue at
/// its price level matches the cached queue order (timestamp, then arrival).
///
/// The downstream book queues each `Add` at the back of its level. The orders at the level that
/// rank behind the new one form the affected suffix:
/// - None: the `Add` is appended.
/// - All last added earlier in this batch, after any earlier delta for this order: the `Add` is
///   placed ahead of the first of their adds, which needs no extra deltas.
/// - Otherwise: the suffix orders are deleted, the order is added, and they are re-added behind
///   it, keeping their cached timestamps and arrival order.
fn emit_queued_add(
    batch: &mut L3DeltaBatch,
    add: OrderBookDelta,
    open_orders: &AHashMap<u64, CachedL3Order>,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<()> {
    let order_id = add.order.order_id;
    let order = open_orders
        .get(&order_id)
        .context("Order missing from the L3 cache when queueing its Add")?;

    let mut suffix: Vec<(u64, &CachedL3Order)> = open_orders
        .iter()
        .filter(|(other_id, other)| {
            **other_id != order_id
                && other.side == order.side
                && other.price == order.price
                && compare_l3_orders(other, order) == Ordering::Greater
        })
        .map(|(other_id, other)| (*other_id, other))
        .collect();

    if suffix.is_empty() {
        batch.push(add);
        return Ok(());
    }

    let floor = batch
        .latest
        .get(&order_id)
        .map_or(0, |(latest_index, _)| latest_index + 1);
    let first_add = suffix
        .iter()
        .map(|(other_id, _)| match batch.latest.get(other_id) {
            Some(&(latest_index, BookAction::Add)) if latest_index >= floor => Some(latest_index),
            _ => None,
        })
        .collect::<Option<Vec<usize>>>()
        .and_then(|indexes| indexes.into_iter().min());

    if let Some(index) = first_add {
        batch.insert(index, add);
        return Ok(());
    }

    suffix.sort_by(|(_, a), (_, b)| compare_l3_orders(a, b));

    for &(other_id, other) in &suffix {
        batch.push(cached_order_delta(
            BookAction::Delete,
            other_id,
            other,
            &add,
            price_precision,
            size_precision,
        )?);
    }
    batch.push(add);
    for &(other_id, other) in &suffix {
        batch.push(cached_order_delta(
            BookAction::Add,
            other_id,
            other,
            &add,
            price_precision,
            size_precision,
        )?);
    }

    Ok(())
}

/// Builds a delta for a cached order, taking the instrument and timestamps from `template`.
fn cached_order_delta(
    action: BookAction,
    order_id: u64,
    cached: &CachedL3Order,
    template: &OrderBookDelta,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<OrderBookDelta> {
    let price = Price::from_decimal_dp(cached.price, price_precision)
        .with_context(|| format!("Failed to construct Price with precision {price_precision}"))?;
    let size = Quantity::from_decimal_dp(cached.size, size_precision)
        .with_context(|| format!("Failed to construct Quantity with precision {size_precision}"))?;
    let book_order = BookOrder::new(cached.side, price, size, order_id);
    OrderBookDelta::new_checked(
        template.instrument_id,
        action,
        book_order,
        0,
        0,
        template.ts_event,
        template.ts_init,
    )
    .with_context(|| format!("Failed to construct {action:?} OrderBookDelta for L3 requeue"))
}

fn next_arrival(arrival: &mut u64) -> u64 {
    let value = *arrival;
    *arrival += 1;
    value
}

#[expect(clippy::too_many_arguments)]
fn append_depth_prune_deltas(
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    depth: u32,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    open_orders: &mut AHashMap<u64, CachedL3Order>,
    batch: &mut L3DeltaBatch,
) -> anyhow::Result<()> {
    if depth == 0 || open_orders.is_empty() {
        return Ok(());
    }

    let mut ask_levels: AHashSet<Decimal> = AHashSet::new();
    let mut bid_levels: AHashSet<Decimal> = AHashSet::new();

    for order in open_orders.values() {
        match order.side {
            OrderSide::Sell => {
                ask_levels.insert(order.price);
            }
            OrderSide::Buy => {
                bid_levels.insert(order.price);
            }
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
            OrderSide::Sell => !ask_keep.contains(&order.price),
            OrderSide::Buy => !bid_keep.contains(&order.price),
        })
        .map(|(order_id, order)| (*order_id, order.clone()))
        .collect();

    pruned.sort_by(|(_, a), (_, b)| compare_l3_orders(a, b));

    for (order_id, cached) in pruned {
        open_orders.remove(&order_id);

        let price = Price::from_decimal_dp(cached.price, price_precision).with_context(|| {
            format!("Failed to construct Price with precision {price_precision}")
        })?;
        let size = Quantity::from_decimal_dp(cached.size, size_precision).with_context(|| {
            format!("Failed to construct Quantity with precision {size_precision}")
        })?;
        let book_order = BookOrder::new(cached.side, price, size, order_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Delete,
            book_order,
            0,
            0,
            ts_event,
            ts_init,
        )
        .context("Failed to construct Delete OrderBookDelta for L3 depth pruning")?;
        batch.push(delta);
    }

    Ok(())
}

fn retained_price_levels(
    open_orders: &AHashMap<u64, CachedL3Order>,
    side: OrderSide,
    depth: u32,
) -> AHashSet<Decimal> {
    let mut levels: Vec<Decimal> = open_orders
        .values()
        .filter(|order| order.side == side)
        .map(|order| order.price)
        .collect();

    match side {
        OrderSide::Sell => levels.sort(),
        OrderSide::Buy => levels.sort_by(|a, b| b.cmp(a)),
    }

    levels.dedup();
    levels.into_iter().take(depth as usize).collect()
}

fn compare_l3_orders(a: &CachedL3Order, b: &CachedL3Order) -> Ordering {
    let side_order = side_rank(a.side).cmp(&side_rank(b.side));
    if side_order != Ordering::Equal {
        return side_order;
    }

    let price_order = match a.side {
        OrderSide::Sell => a.price.cmp(&b.price),
        OrderSide::Buy => b.price.cmp(&a.price),
    };

    price_order
        .then(a.timestamp.cmp(&b.timestamp))
        .then(a.seq.cmp(&b.seq))
}

fn side_rank(side: OrderSide) -> u8 {
    match side {
        OrderSide::Sell => 0,
        OrderSide::Buy => 1,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        enums::{BookAction, BookType, RecordFlag},
        identifiers::{InstrumentId, Symbol},
        instruments::{InstrumentAny, currency_pair::CurrencyPair},
        orderbook::{BookLevel, OrderBook},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::value::RawValue;

    use super::*;
    use crate::websocket::spot_v2::level_3::checksum::{build_checksum_string, compute_checksum};

    #[derive(serde::Deserialize)]
    struct RawDataMessage {
        data: Vec<Box<RawValue>>,
    }

    fn make_instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from("BTC/USD.KRAKEN"))
                .raw_symbol(Symbol::from("BTC/USD"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .price_precision(1)
                .size_precision(8)
                .price_increment(Price::from("0.1"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn load_snapshot() -> KrakenL3Snapshot {
        let json = include_str!("../../../../test_data/ws_l3_snapshot.json");
        let message: RawDataMessage = serde_json::from_str(json).unwrap();
        serde_json::from_str(message.data[0].get()).unwrap()
    }

    fn load_update(filename: &str) -> KrakenL3UpdateData {
        let path = format!("{}/test_data/{filename}", env!("CARGO_MANIFEST_DIR"));
        let json = std::fs::read_to_string(&path).unwrap();
        let message: RawDataMessage = serde_json::from_str(&json).unwrap();
        serde_json::from_str(message.data[0].get()).unwrap()
    }

    /// Returns the cache after `ws_l3_snapshot.json`, and the arrival counter to continue from.
    fn populated_open_orders() -> (AHashMap<u64, CachedL3Order>, u64) {
        let snap = load_snapshot();
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut arrival = 0u64;
        let mut open_orders = AHashMap::new();
        parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
            UnixNanos::default(),
            &mut open_orders,
        )
        .unwrap();
        (open_orders, arrival)
    }

    #[rstest]
    fn test_parse_l3_snapshot_delta_count() {
        let snap = load_snapshot();
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let mut arrival = 0u64;
        let mut open_orders = AHashMap::new();

        let result = parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let mut arrival = 0u64;
        let mut open_orders = AHashMap::new();

        let result = parse_l3_snapshot(
            &snap,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let (mut open_orders, mut arrival) = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let (mut open_orders, mut arrival) = populated_open_orders();
        let pruned_order_id = hasher.hash("order-bid-3");

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let (mut open_orders, mut arrival) = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let (mut open_orders, mut arrival) = populated_open_orders();
        let order_id = hasher.hash("order-ask-1");
        let cached_raw = open_orders
            .get(&order_id)
            .expect("seeded order-ask-1 in snapshot")
            .price_raw
            .clone();
        assert_eq!(cached_raw, "42001.0");

        // Deserialize the data payload directly so the wire-format decimal text is preserved
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
        assert_eq!(update.asks[0].limit_price.value, dec!(42001));

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let (mut open_orders, mut arrival) = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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

    fn bid_1_modify_update(order_qty: &str) -> KrakenL3UpdateData {
        let json = format!(
            r#"{{
                "symbol": "BTC/USD",
                "timestamp": "2024-01-15T12:00:02.000000Z",
                "checksum": 0,
                "bids": [
                    {{
                        "event": "modify",
                        "order_id": "order-bid-1",
                        "limit_price": 42000.5,
                        "order_qty": {order_qty},
                        "timestamp": "2024-01-15T12:00:02.000000Z"
                    }}
                ],
                "asks": []
            }}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[rstest]
    #[case::size_increase("0.9", "4200108420020204200051242000594199903")]
    #[case::size_unchanged("0.5", "4200108420020204200051242000554199903")]
    fn test_parse_l3_update_modify_without_size_decrease_moves_to_back_of_level(
        #[case] order_qty: &str,
        #[case] expected_checksum_string: &str,
    ) {
        let update = bid_1_modify_update(order_qty);
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 100u64;
        let (mut open_orders, mut arrival) = populated_open_orders();
        let bid_1 = hasher.hash("order-bid-1");
        let bid_2 = hasher.hash("order-bid-2");
        assert!(open_orders[&bid_1].seq < open_orders[&bid_2].seq);

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 2);
        assert_eq!(result.deltas[0].action, BookAction::Delete);
        assert_eq!(result.deltas[0].order.size, Quantity::from("0.5"));
        assert_eq!(result.deltas[1].action, BookAction::Add);
        assert_eq!(result.deltas[1].order.size, Quantity::from(order_qty));
        assert!(open_orders[&bid_1].seq > open_orders[&bid_2].seq);

        // Bids at 42000.5 now checksum as order-bid-2 then order-bid-1
        assert_eq!(
            build_checksum_string(&open_orders),
            expected_checksum_string
        );
    }

    #[rstest]
    fn test_parse_l3_update_modify_size_decrease_keeps_priority() {
        let update = bid_1_modify_update("0.4");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 100u64;
        let (mut open_orders, mut arrival) = populated_open_orders();
        let bid_1 = hasher.hash("order-bid-1");
        let seq_before = open_orders[&bid_1].seq;

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();
        let result = deltas.unwrap();

        assert_eq!(result.deltas.len(), 1);
        assert_eq!(result.deltas[0].action, BookAction::Update);
        assert_eq!(open_orders[&bid_1].seq, seq_before);
    }

    #[rstest]
    fn test_parse_l3_update_delete() {
        let update = load_update("ws_l3_update_delete.json");
        let instrument = make_instrument();
        let hasher = BookOrderIdHasher::new();
        let mut sequence = 0u64;
        let (mut open_orders, mut arrival) = populated_open_orders();

        let (deltas, _ts_event) = parse_l3_update(
            &update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
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
        let mut arrival = 0u64;
        let mut open_orders = AHashMap::new();

        let empty_update = KrakenL3UpdateData {
            symbol: "BTC/USD".to_string(),
            bids: vec![],
            asks: vec![],
            checksum: 0,
            timestamp: jiff::Timestamp::now(),
        };

        let (deltas, _ts_event) = parse_l3_update(
            &empty_update,
            &instrument,
            &hasher,
            &mut sequence,
            &mut arrival,
            UnixNanos::default(),
            &mut open_orders,
            1000,
        )
        .unwrap();

        assert!(deltas.is_none());
    }

    fn make_eth_instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from("ETH/USD.KRAKEN"))
                .raw_symbol(Symbol::from("ETH/USD"))
                .base_currency(Currency::ETH())
                .quote_currency(Currency::USD())
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn load_snapshot_file(filename: &str) -> KrakenL3Snapshot {
        let path = format!("{}/test_data/{filename}", env!("CARGO_MANIFEST_DIR"));
        let json = std::fs::read_to_string(&path).unwrap();
        let message: RawDataMessage = serde_json::from_str(&json).unwrap();
        serde_json::from_str(message.data[0].get()).unwrap()
    }

    /// A snapshot order, timestamped `second` seconds past 12:00.
    fn order_json(order_id: &str, price: &str, qty: &str, second: u32) -> String {
        format!(
            r#"{{"order_id": "{order_id}", "limit_price": {price}, "order_qty": {qty}, "timestamp": "2024-01-15T12:00:{second:02}.000000Z"}}"#
        )
    }

    /// An update event, timestamped `second` seconds past 12:00.
    fn event_json(event: &str, order_id: &str, price: &str, qty: &str, second: u32) -> String {
        format!(
            r#"{{"event": "{event}", "order_id": "{order_id}", "limit_price": {price}, "order_qty": {qty}, "timestamp": "2024-01-15T12:00:{second:02}.000000Z"}}"#
        )
    }

    fn synthetic_snapshot(bids: &[String]) -> KrakenL3Snapshot {
        let json = format!(
            r#"{{"symbol": "BTC/USD", "timestamp": "2024-01-15T12:00:59.000000Z", "checksum": 0, "bids": [{}], "asks": []}}"#,
            bids.join(", ")
        );
        serde_json::from_str(&json).unwrap()
    }

    fn synthetic_update(bids: &[String]) -> KrakenL3UpdateData {
        let json = format!(
            r#"{{"symbol": "BTC/USD", "timestamp": "2024-01-15T12:01:00.000000Z", "checksum": 0, "bids": [{}], "asks": []}}"#,
            bids.join(", ")
        );
        serde_json::from_str(&json).unwrap()
    }

    /// Parses batches, applies them to a real `OrderBook`, and checks that every downstream
    /// queue matches the cached queue order after each batch.
    struct L3Replay {
        instrument: InstrumentAny,
        hasher: BookOrderIdHasher,
        sequence: u64,
        arrival: u64,
        open_orders: AHashMap<u64, CachedL3Order>,
        book: OrderBook,
        depth: u32,
    }

    impl L3Replay {
        fn new(instrument: InstrumentAny, depth: u32) -> Self {
            let book = OrderBook::new(instrument.id(), BookType::L3_MBO);
            Self {
                instrument,
                hasher: BookOrderIdHasher::new(),
                sequence: 0,
                arrival: 0,
                open_orders: AHashMap::new(),
                book,
                depth,
            }
        }

        fn snapshot(&mut self, snap: &KrakenL3Snapshot) -> OrderBookDeltas {
            self.open_orders.clear();
            let deltas = parse_l3_snapshot(
                snap,
                &self.instrument,
                &self.hasher,
                &mut self.sequence,
                &mut self.arrival,
                UnixNanos::default(),
                &mut self.open_orders,
            )
            .unwrap();
            self.apply(&deltas);
            deltas
        }

        fn update(&mut self, update: &KrakenL3UpdateData) -> OrderBookDeltas {
            let (deltas, _ts_event) = parse_l3_update(
                update,
                &self.instrument,
                &self.hasher,
                &mut self.sequence,
                &mut self.arrival,
                UnixNanos::default(),
                &mut self.open_orders,
                self.depth,
            )
            .unwrap();
            let deltas = deltas.expect("update should emit deltas");
            self.apply(&deltas);
            deltas
        }

        fn apply(&mut self, deltas: &OrderBookDeltas) {
            let first = deltas.deltas[0].sequence;
            for (i, delta) in deltas.deltas.iter().enumerate() {
                assert_eq!(delta.sequence, first + i as u64, "sequence out of order");
            }
            self.book.apply_deltas(deltas).unwrap();
            self.assert_book_matches_cache();
        }

        fn assert_book_matches_cache(&self) {
            let mut cached: Vec<(&u64, &CachedL3Order)> = self.open_orders.iter().collect();
            cached.sort_by(|(_, a), (_, b)| compare_l3_orders(a, b));
            let expected: Vec<(u64, Decimal)> =
                cached.iter().map(|(id, o)| (**id, o.size)).collect();

            // Same order as `compare_l3_orders`: asks ascending, then bids descending
            let actual: Vec<(u64, Decimal)> = self
                .book
                .asks(None)
                .chain(self.book.bids(None))
                .flat_map(BookLevel::iter)
                .map(|o| (o.order_id, o.size.as_decimal()))
                .collect();

            assert_eq!(actual, expected, "downstream queues differ from the cache");
        }

        fn bid_queue(&self, price: &str) -> Vec<u64> {
            let price = Price::from(price);
            self.book
                .bids(None)
                .find(|level| level.price.value == price)
                .map_or_else(Vec::new, |level| level.iter().map(|o| o.order_id).collect())
        }

        fn ids(&self, names: &[&str]) -> Vec<u64> {
            names.iter().map(|name| self.hasher.hash(name)).collect()
        }

        fn actions(&self, expected: &[(BookAction, &str)]) -> Vec<(BookAction, u64)> {
            expected
                .iter()
                .map(|(action, name)| (*action, self.hasher.hash(name)))
                .collect()
        }
    }

    fn emitted(deltas: &OrderBookDeltas) -> Vec<(BookAction, u64)> {
        deltas
            .deltas
            .iter()
            .map(|d| (d.action, d.order.order_id))
            .collect()
    }

    #[rstest]
    fn test_recorded_add_queued_by_timestamp_matches_kraken_checksums() {
        // Recorded on ETH/USD on 2026-10-21: the book before the add (rebuilt from the stream),
        // then Kraken's add and the modify that followed it, with Kraken's checksums
        let mut replay = L3Replay::new(make_eth_instrument(), 1000);
        let snap = load_snapshot_file("ws_l3_snapshot_queued_by_timestamp.json");
        replay.snapshot(&snap);
        assert_eq!(compute_checksum(&replay.open_orders), snap.checksum);

        // The add is older than the five orders resting at its level, so it joins ahead of them
        let add = load_update("ws_l3_update_add_queued_by_timestamp.json");
        let deltas = replay.update(&add);
        assert_eq!(compute_checksum(&replay.open_orders), add.checksum);
        let resting = [
            "OFWTEM-GONLD-WJ2ASM",
            "OX3WTW-VLOVV-QVB5YX",
            "O6TZBM-QJJ65-4MKKEY",
            "O7SRKK-PNOE2-U6FSXT",
            "OQDO2V-BFC5V-TZ3Y44",
        ];
        let new = "OJZ4KN-PWGS4-45HCTC";
        assert_eq!(deltas.deltas.len(), 11); // 5 deletes, the add, 5 re-adds
        let level = replay
            .book
            .asks(None)
            .find(|level| level.price.value == Price::from("2478.76"))
            .unwrap();
        let queue: Vec<u64> = level.iter().map(|o| o.order_id).collect();
        let mut expected = vec![new];
        expected.extend(resting);
        assert_eq!(queue, replay.ids(&expected));

        // The modify keeps the size, so Kraken requeues the order at the back
        let modify = load_update("ws_l3_update_modify_queued_by_timestamp.json");
        let deltas = replay.update(&modify);
        assert_eq!(compute_checksum(&replay.open_orders), modify.checksum);
        assert_eq!(
            emitted(&deltas),
            replay.actions(&[(BookAction::Delete, new), (BookAction::Add, new)])
        );
    }

    #[rstest]
    fn test_snapshot_emits_levels_in_timestamp_order() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-b", "42000.0", "0.2", 20),
            order_json("bid-a", "42000.0", "0.1", 10),
            order_json("bid-c", "42000.0", "0.3", 20),
        ]));

        // Equal timestamps keep snapshot order
        assert_eq!(
            replay.bid_queue("42000.0"),
            replay.ids(&["bid-a", "bid-b", "bid-c"])
        );
    }

    #[rstest]
    fn test_adds_in_one_update_are_emitted_in_queue_order() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[order_json(
            "bid-1", "42000.0", "1.0", 1,
        )]));

        let deltas = replay.update(&synthetic_update(&[
            event_json("add", "bid-c", "41990.0", "0.3", 30),
            event_json("add", "bid-a", "41990.0", "0.1", 10),
            event_json("add", "bid-b", "41990.0", "0.2", 20),
        ]));

        // Reordered within the batch, so no extra deltas
        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Add, "bid-a"),
                (BookAction::Add, "bid-b"),
                (BookAction::Add, "bid-c"),
            ])
        );
        assert_eq!(
            replay.bid_queue("41990.0"),
            replay.ids(&["bid-a", "bid-b", "bid-c"])
        );
    }

    #[rstest]
    fn test_add_ahead_of_resting_orders_requeues_suffix() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-p", "42000.0", "0.1", 10),
            order_json("bid-q", "42000.0", "0.2", 30),
            order_json("bid-r", "42000.0", "0.3", 40),
        ]));

        let deltas = replay.update(&synthetic_update(&[event_json(
            "add", "bid-x", "42000.0", "0.5", 20,
        )]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Delete, "bid-q"),
                (BookAction::Delete, "bid-r"),
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-q"),
                (BookAction::Add, "bid-r"),
            ])
        );
        assert_eq!(
            replay.bid_queue("42000.0"),
            replay.ids(&["bid-p", "bid-x", "bid-q", "bid-r"])
        );
    }

    #[rstest]
    fn test_timestamp_ties_keep_original_arrival_order() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-p", "42000.0", "0.1", 10),
            order_json("bid-q", "42000.0", "0.2", 10),
        ]));

        // Requeue p and q behind an older add; they keep their arrival order
        replay.update(&synthetic_update(&[event_json(
            "add", "bid-x", "42000.0", "0.5", 5,
        )]));

        // A later arrival with the same timestamp queues behind both, despite their re-adds
        // carrying newer delta sequences than this order's arrival
        let deltas = replay.update(&synthetic_update(&[
            event_json("add", "bid-y", "42000.0", "0.3", 10),
            event_json("add", "bid-z", "42000.0", "0.4", 10),
        ]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[(BookAction::Add, "bid-y"), (BookAction::Add, "bid-z")])
        );
        assert_eq!(
            replay.bid_queue("42000.0"),
            replay.ids(&["bid-x", "bid-p", "bid-q", "bid-y", "bid-z"])
        );
    }

    #[rstest]
    fn test_level_reentering_depth_window_is_queued_by_timestamp() {
        let mut replay = L3Replay::new(make_instrument(), 2);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-p", "42000.0", "0.1", 1),
            order_json("bid-r", "41990.0", "0.2", 5),
            order_json("bid-s", "41990.0", "0.3", 15),
        ]));

        // A better level pushes 41990.0 out of the depth window
        replay.update(&synthetic_update(&[event_json(
            "add", "bid-n", "42010.0", "0.4", 40,
        )]));
        assert!(replay.bid_queue("41990.0").is_empty());

        // When it comes back, Kraken resends its orders, here out of queue order
        let deltas = replay.update(&synthetic_update(&[
            event_json("delete", "bid-n", "42010.0", "0", 41),
            event_json("add", "bid-s", "41990.0", "0.3", 15),
            event_json("add", "bid-r", "41990.0", "0.2", 5),
        ]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Delete, "bid-n"),
                (BookAction::Add, "bid-r"),
                (BookAction::Add, "bid-s"),
            ])
        );
        assert_eq!(replay.bid_queue("41990.0"), replay.ids(&["bid-r", "bid-s"]));
    }

    #[rstest]
    fn test_mixed_update_keeps_events_for_a_reordered_order_after_its_add() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[order_json(
            "bid-1", "42000.0", "1.0", 1,
        )]));

        let deltas = replay.update(&synthetic_update(&[
            event_json("add", "bid-b", "41990.0", "0.2", 20),
            event_json("add", "bid-x", "41990.0", "0.5", 10),
            event_json("modify", "bid-x", "41990.0", "0.4", 10),
            event_json("add", "bid-y", "41990.0", "0.6", 5),
            event_json("delete", "bid-x", "41990.0", "0", 10),
        ]));

        // x moves ahead of b's add. Once x has an update in the batch, y cannot move ahead of
        // x's add, so x and b are requeued behind y, and x's delete still follows its re-add
        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-b"),
                (BookAction::Update, "bid-x"),
                (BookAction::Delete, "bid-x"),
                (BookAction::Delete, "bid-b"),
                (BookAction::Add, "bid-y"),
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-b"),
                (BookAction::Delete, "bid-x"),
            ])
        );
        assert_eq!(replay.bid_queue("41990.0"), replay.ids(&["bid-y", "bid-b"]));
    }

    #[rstest]
    fn test_mixed_update_add_ahead_of_requeued_modify() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-p", "42000.0", "0.1", 10),
            order_json("bid-q", "42000.0", "0.2", 20),
        ]));

        // p's size increase requeues it behind q; x then lands between q and p
        let deltas = replay.update(&synthetic_update(&[
            event_json("modify", "bid-p", "42000.0", "0.15", 50),
            event_json("add", "bid-x", "42000.0", "0.5", 30),
        ]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Delete, "bid-p"),
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-p"),
            ])
        );
        assert_eq!(
            replay.bid_queue("42000.0"),
            replay.ids(&["bid-q", "bid-x", "bid-p"])
        );
    }

    #[rstest]
    fn test_mixed_update_add_ahead_of_size_decrease_requeues_with_new_size() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[
            order_json("bid-p", "42000.0", "0.1", 10),
            order_json("bid-q", "42000.0", "0.2", 40),
        ]));

        // q keeps its place on the size decrease, then is re-added behind x at its new size
        let deltas = replay.update(&synthetic_update(&[
            event_json("modify", "bid-q", "42000.0", "0.15", 40),
            event_json("add", "bid-x", "42000.0", "0.5", 20),
        ]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Update, "bid-q"),
                (BookAction::Delete, "bid-q"),
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-q"),
            ])
        );
        assert_eq!(deltas.deltas[3].order.size, Quantity::from("0.15"));
        assert_eq!(
            replay.bid_queue("42000.0"),
            replay.ids(&["bid-p", "bid-x", "bid-q"])
        );
    }

    #[rstest]
    fn test_mixed_update_price_change_does_not_move_ahead_of_its_own_delete() {
        let mut replay = L3Replay::new(make_instrument(), 1000);
        replay.snapshot(&synthetic_snapshot(&[order_json(
            "bid-x", "42000.0", "0.5", 10,
        )]));

        // x moves to 41990.0 after y was added there; its add cannot precede its own delete,
        // so y is requeued behind it instead
        let deltas = replay.update(&synthetic_update(&[
            event_json("add", "bid-y", "41990.0", "0.2", 30),
            event_json("modify", "bid-x", "41990.0", "0.5", 20),
        ]));

        assert_eq!(
            emitted(&deltas),
            replay.actions(&[
                (BookAction::Add, "bid-y"),
                (BookAction::Delete, "bid-x"),
                (BookAction::Delete, "bid-y"),
                (BookAction::Add, "bid-x"),
                (BookAction::Add, "bid-y"),
            ])
        );
        assert_eq!(replay.bid_queue("41990.0"), replay.ids(&["bid-x", "bid-y"]));
    }
}
