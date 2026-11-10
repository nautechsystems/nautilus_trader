#![no_main]

use std::{collections::BTreeMap, time::Duration};

use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::UnixNanos;
use nautilus_kalshi::{
    book::BookSyncTracker,
    config::KalshiDataClientConfig,
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        messages::{KalshiChannel, KalshiSubscribed, KalshiWsMessage},
        parse::decode_frame,
    },
};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    enums::{BookAction, OrderSide},
    identifiers::InstrumentId,
    instruments::BinaryOption,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};

#[path = "../common/mod.rs"]
mod common;

const SID: u64 = 7;
const GENERATION: u64 = 4;

fuzz_target!(|data: &[u8]| {
    let Some(data) = common::capped(data) else {
        return;
    };
    let instrument = common::instrument(Some("MARKET-A"));
    let id = instrument.id;
    let instruments = common::instruments(instrument);
    let tracker = BookSyncTracker::new(&[id], Duration::from_secs(10));
    tracker.subscribe(id, None).unwrap();
    let (ws_client, _events) =
        KalshiWebSocketClient::new(KalshiDataClientConfig::default()).unwrap();
    tracker.replay(&ws_client).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let sender: EventSender<DataEvent> = sender.into();
    let now = UnixNanos::from(1);
    tracker
        .event(&subscribed(GENERATION), &instruments, &sender, now)
        .unwrap();

    let books: Value = serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();
    let snapshot = apply(
        &tracker,
        &instruments,
        &sender,
        &mut receiver,
        &frame("orderbook_snapshot", &books["snapshot"], 1),
        GENERATION,
        now,
    );
    let snapshot = snapshot.expect("fixture snapshot must emit");
    assert_batch(&snapshot);
    let mut published = PublishedBook::from_batch(&snapshot);

    let mut generation = GENERATION;
    let mut synced = true;

    for (index, op) in data.iter().enumerate() {
        match op % 5 {
            0 => {
                let Some(target) = published.reduction_target(index) else {
                    continue;
                };
                let emitted = apply(
                    &tracker,
                    &instruments,
                    &sender,
                    &mut receiver,
                    &frame(
                        "orderbook_delta",
                        &reduction_payload(&target),
                        2 + index as u64,
                    ),
                    generation.saturating_sub(1),
                    now,
                );
                assert!(emitted.is_none(), "older generation emitted a batch");
            }
            1 => {
                tracker
                    .event(
                        &KalshiWsEvent::BookSequenceGap {
                            generation,
                            sid: SID,
                        },
                        &instruments,
                        &sender,
                        now,
                    )
                    .unwrap();
                synced = false;
            }
            2 => {
                generation = generation.saturating_add(1);
                tracker
                    .event(&subscribed(generation), &instruments, &sender, now)
                    .unwrap();
                let suppressed = apply(
                    &tracker,
                    &instruments,
                    &sender,
                    &mut receiver,
                    &frame(
                        "orderbook_delta",
                        &add_payload(*op, index),
                        2 + index as u64,
                    ),
                    generation,
                    now,
                );
                assert!(
                    suppressed.is_none(),
                    "delta before replacement snapshot emitted"
                );
                let emitted = apply(
                    &tracker,
                    &instruments,
                    &sender,
                    &mut receiver,
                    &frame("orderbook_snapshot", &books["snapshot"], 1),
                    generation,
                    now,
                );
                let batch = emitted.expect("current-generation snapshot must emit");
                assert_batch(&batch);
                published = PublishedBook::from_batch(&batch);
                synced = true;
            }
            3 => {
                let seq = 2 + index as u64;
                if synced && index.is_multiple_of(4) {
                    let emitted = apply(
                        &tracker,
                        &instruments,
                        &sender,
                        &mut receiver,
                        &frame("orderbook_delta", &invalid_payload(*op, &published), seq),
                        generation,
                        now,
                    );
                    assert!(emitted.is_none(), "invalid book delta emitted a batch");
                    synced = false;
                } else if synced
                    && published.reduction_target(index).is_some()
                    && !op.is_multiple_of(2)
                {
                    let target = published.reduction_target(index).unwrap();
                    let expected = target.qty - REDUCTION;
                    let emitted = apply(
                        &tracker,
                        &instruments,
                        &sender,
                        &mut receiver,
                        &frame("orderbook_delta", &reduction_payload(&target), seq),
                        generation,
                        now,
                    );
                    let batch = emitted.expect("synced reduction must emit");
                    assert_reduction(&batch, &target, expected);
                    published.apply_batch(&batch);
                    synced = true;
                } else if synced
                    && published.reduction_target(index).is_some()
                    && index.is_multiple_of(3)
                {
                    let target = published.reduction_target(index).unwrap();
                    let emitted = apply(
                        &tracker,
                        &instruments,
                        &sender,
                        &mut receiver,
                        &frame("orderbook_delta", &increase_payload(&target), seq),
                        generation,
                        now,
                    );
                    let batch = emitted.expect("synced add must emit");
                    assert_increase(&batch, &target);
                    published.apply_batch(&batch);
                } else if synced {
                    let probe = level_probe(*op, index, &published);
                    let emitted = apply(
                        &tracker,
                        &instruments,
                        &sender,
                        &mut receiver,
                        &frame("orderbook_delta", &probe.payload, seq),
                        generation,
                        now,
                    );

                    if probe.crosses {
                        assert!(emitted.is_none(), "crossed add emitted a batch");
                        synced = false;
                    } else {
                        let batch = emitted.expect("synced add must emit");
                        assert_level(&batch, &probe);
                        published.apply_batch(&batch);
                    }
                } else {
                    let emitted = apply(
                        &tracker,
                        &instruments,
                        &sender,
                        &mut receiver,
                        &frame("orderbook_delta", &add_payload(*op, index), seq),
                        generation,
                        now,
                    );
                    assert!(emitted.is_none(), "gapped book emitted a delta");
                }
            }
            _ => {
                let bad = include_bytes!("../../test_data/orderbook_delta_incomplete.json");
                let message = decode_frame(bad).unwrap_or(KalshiWsMessage::Unknown);
                tracker
                    .event(
                        &KalshiWsEvent::Message {
                            generation,
                            message: message.clone(),
                        },
                        &instruments,
                        &sender,
                        now,
                    )
                    .unwrap();
                assert!(
                    recv(&mut receiver).is_none(),
                    "invalid delta emitted a batch"
                );
                tracker
                    .event(
                        &KalshiWsEvent::Message {
                            generation,
                            message,
                        },
                        &instruments,
                        &sender,
                        now,
                    )
                    .unwrap();
                assert!(
                    recv(&mut receiver).is_none(),
                    "invalid delta rejection changed"
                );
                synced = false;
            }
        }
    }
});

fn subscribed(generation: u64) -> KalshiWsEvent {
    KalshiWsEvent::Message {
        generation,
        message: KalshiWsMessage::Subscribed {
            id: 1,
            msg: KalshiSubscribed {
                channel: KalshiChannel::OrderbookDelta,
                sid: SID,
            },
        },
    }
}

fn frame(kind: &str, payload: &Value, seq: u64) -> Vec<u8> {
    json!({
        "type": kind,
        "sid": SID,
        "seq": seq,
        "sending_ts_ms": 1_700_000_000_001u64,
        "msg": payload,
    })
    .to_string()
    .into_bytes()
}

const REDUCTION: Decimal = Decimal::from_parts(25, 0, 0, false, 2);

struct Level {
    side: &'static str,
    order_side: OrderSide,
    wire_price: &'static str,
    price: Decimal,
    qty: Decimal,
}

struct PublishedBook {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

impl PublishedBook {
    fn from_batch(batch: &OrderBookDeltas) -> Self {
        let mut book = Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        };

        book.apply_batch(batch);
        book.assert_uncrossed();
        book
    }

    fn apply_batch(&mut self, batch: &OrderBookDeltas) {
        for delta in &batch.deltas {
            if delta.action == BookAction::Clear {
                self.bids.clear();
                self.asks.clear();
                continue;
            }

            let price = delta.order.price.as_decimal();
            let size = delta.order.size.as_decimal();

            let levels = if delta.order.side == Some(OrderSide::Buy) {
                &mut self.bids
            } else {
                &mut self.asks
            };

            if delta.action == BookAction::Delete || size.is_zero() {
                levels.remove(&price);
            } else {
                levels.insert(price, size);
            }
        }

        self.assert_uncrossed();
    }

    fn assert_uncrossed(&self) {
        if let (Some((bid, _)), Some((ask, _))) =
            (self.bids.last_key_value(), self.asks.first_key_value())
        {
            assert!(bid < ask, "emitted book is crossed or locked");
        }
    }

    fn reduction_target(&self, index: usize) -> Option<Level> {
        let mut levels = Vec::new();

        for (side, order_side, wire_price) in [
            ("yes", OrderSide::Buy, "0.123456"),
            ("yes", OrderSide::Buy, "0.213456"),
            ("no", OrderSide::Sell, "0.713456"),
            ("no", OrderSide::Sell, "0.813456"),
        ] {
            let price = Decimal::from_str_exact(wire_price).unwrap();
            let qty = if order_side == OrderSide::Buy {
                self.bids.get(&price).copied()
            } else {
                self.asks.get(&price).copied()
            };

            if qty.is_some_and(|qty| qty >= REDUCTION) {
                levels.push(Level {
                    side,
                    order_side,
                    wire_price,
                    price,
                    qty: qty.unwrap(),
                });
            }
        }

        if levels.is_empty() {
            None
        } else {
            Some(levels.swap_remove(index % levels.len()))
        }
    }
}

fn reduction_payload(target: &Level) -> Value {
    json!({
        "market_ticker": "MARKET-A",
        "side": target.side,
        "price_dollars": target.wire_price,
        "delta_fp": "-0.25",
    })
}

fn increase_payload(target: &Level) -> Value {
    json!({
        "market_ticker": "MARKET-A",
        "side": target.side,
        "price_dollars": target.wire_price,
        "delta_fp": "1.00",
    })
}

fn invalid_payload(op: u8, published: &PublishedBook) -> Value {
    let (side, price, delta) = match op % 4 {
        0 => published.reduction_target(0).map_or_else(
            || ("yes".to_owned(), "1.000001".to_owned(), "1.00".to_owned()),
            |target| {
                (
                    target.side.to_owned(),
                    target.wire_price.to_owned(),
                    format!("-{}", target.qty + REDUCTION),
                )
            },
        ),
        1 => ("yes".to_owned(), "0.123456".to_owned(), "0.00".to_owned()),
        2 => ("yes".to_owned(), "1.000001".to_owned(), "1.00".to_owned()),
        _ => ("yes".to_owned(), "0.1234567".to_owned(), "1.00".to_owned()),
    };

    json!({
        "market_ticker": "MARKET-A",
        "side": side,
        "price_dollars": price,
        "delta_fp": delta,
    })
}

struct LevelProbe {
    payload: Value,
    crosses: bool,
    price: Decimal,
    order_side: OrderSide,
    previous: Option<Decimal>,
}

fn level_probe(op: u8, index: usize, book: &PublishedBook) -> LevelProbe {
    let bid = index.is_multiple_of(2);
    let cross = (op / 2).is_multiple_of(2);
    let (price, crosses, order_side, previous) = if bid {
        bid_probe(book, cross)
    } else {
        ask_probe(book, cross)
    };

    let side = if bid { "yes" } else { "no" };

    LevelProbe {
        payload: json!({
            "market_ticker": "MARKET-A",
            "side": side,
            "price_dollars": price.to_string(),
            "delta_fp": "1.00",
        }),
        crosses,
        price,
        order_side,
        previous,
    }
}

fn bid_probe(book: &PublishedBook, cross: bool) -> (Decimal, bool, OrderSide, Option<Decimal>) {
    let Some((ask, _)) = book.asks.first_key_value().map(|(price, _)| (*price, ())) else {
        let price = Decimal::new(1, 2);
        return (price, false, OrderSide::Buy, book.bids.get(&price).copied());
    };

    if cross {
        return (ask, true, OrderSide::Buy, None);
    }

    let mut price = (ask - Decimal::new(1, 6)).max(Decimal::ZERO);
    while book.bids.contains_key(&price) && price > Decimal::ZERO {
        let next = price - Decimal::new(1, 6);
        if next >= ask {
            break;
        }

        price = next.max(Decimal::ZERO);
        if price == Decimal::ZERO {
            break;
        }
    }

    (
        price,
        price >= ask,
        OrderSide::Buy,
        book.bids.get(&price).copied(),
    )
}

fn ask_probe(book: &PublishedBook, cross: bool) -> (Decimal, bool, OrderSide, Option<Decimal>) {
    let Some((bid, _)) = book.bids.last_key_value().map(|(price, _)| (*price, ())) else {
        let price = Decimal::new(99, 2);

        return (
            price,
            false,
            OrderSide::Sell,
            book.asks.get(&price).copied(),
        );
    };

    if cross {
        return (bid, true, OrderSide::Sell, None);
    }

    let mut price = (bid + Decimal::new(1, 6)).min(Decimal::ONE);
    while book.asks.contains_key(&price) && price < Decimal::ONE {
        let next = price + Decimal::new(1, 6);
        if next <= bid {
            break;
        }

        price = next.min(Decimal::ONE);
        if price == Decimal::ONE {
            break;
        }
    }

    (
        price,
        price <= bid,
        OrderSide::Sell,
        book.asks.get(&price).copied(),
    )
}

fn add_payload(op: u8, index: usize) -> Value {
    json!({
        "market_ticker": "MARKET-A",
        "side": if index.is_multiple_of(2) { "yes" } else { "no" },
        "price_dollars": format!("0.{:06}", u32::from(op) * 1000 % 900_000 + 1),
        "delta_fp": "1.00",
    })
}

fn assert_reduction(batch: &OrderBookDeltas, target: &Level, expected: Decimal) {
    let delta = batch
        .deltas
        .iter()
        .find(|delta| delta.action != BookAction::Clear)
        .expect("reduction emitted a level");
    assert_eq!(delta.order.side, Some(target.order_side));
    assert_eq!(delta.order.price.as_decimal(), target.price);
    assert_eq!(delta.order.size.as_decimal(), expected);
    assert_eq!(
        delta.action,
        if expected.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        }
    );
}

fn assert_level(batch: &OrderBookDeltas, probe: &LevelProbe) {
    let delta = batch
        .deltas
        .iter()
        .find(|delta| delta.action != BookAction::Clear)
        .expect("add emitted a level");
    assert_eq!(delta.order.side, Some(probe.order_side));
    assert_eq!(delta.order.price.as_decimal(), probe.price);
    assert_eq!(
        delta.order.size.as_decimal(),
        probe.previous.unwrap_or(Decimal::ZERO) + Decimal::ONE
    );
    assert_eq!(
        delta.action,
        if probe.previous.is_some() {
            BookAction::Update
        } else {
            BookAction::Add
        }
    );
}

fn assert_increase(batch: &OrderBookDeltas, target: &Level) {
    let delta = batch
        .deltas
        .iter()
        .find(|delta| delta.action != BookAction::Clear)
        .expect("add emitted a level");
    assert_eq!(delta.order.side, Some(target.order_side));
    assert_eq!(delta.order.price.as_decimal(), target.price);
    assert_eq!(delta.order.size.as_decimal(), target.qty + Decimal::ONE);
    assert_eq!(delta.action, BookAction::Update);
}

fn apply(
    tracker: &BookSyncTracker,
    instruments: &std::collections::HashMap<InstrumentId, BinaryOption>,
    sender: &EventSender<DataEvent>,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    frame: &[u8],
    generation: u64,
    now: UnixNanos,
) -> Option<OrderBookDeltas> {
    let Ok(message) = decode_frame(frame) else {
        return None;
    };

    tracker
        .event(
            &KalshiWsEvent::Message {
                generation,
                message,
            },
            instruments,
            sender,
            now,
        )
        .unwrap();

    recv(receiver)
}

fn recv(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Option<OrderBookDeltas> {
    match receiver.try_recv().ok()? {
        DataEvent::Data(Data::BookDeltas(batch)) => Some(*batch),
        _ => None,
    }
}

fn assert_batch(batch: &OrderBookDeltas) {
    let mut bids = Vec::new();
    let mut asks = Vec::new();

    for delta in &batch.deltas {
        if delta.action == BookAction::Clear {
            continue;
        }

        let price = delta.order.price.as_decimal();
        let size = delta.order.size.as_decimal();
        assert!(
            price >= Decimal::ZERO && price <= Decimal::ONE,
            "book price left the binary payout interval"
        );
        assert!(size >= Decimal::ZERO, "book size is negative");

        if delta.order.side == Some(OrderSide::Buy) {
            bids.push(price);
        } else {
            asks.push(price);
        }
    }

    if let (Some(bid), Some(ask)) = (bids.iter().max(), asks.iter().min()) {
        assert!(bid < ask, "emitted book is crossed or locked");
    }
}
