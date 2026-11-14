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

//! Shared fixtures for Kalshi criterion benches.
//!
//! Market-data and book payloads come from the adapter's permitted synthetic fixtures. Execution
//! records use the same REST fixtures as the report tests, with direction and counts aligned so
//! the dispatch path accepts them.

#![allow(dead_code)]

use std::{collections::HashMap, hint::black_box};

use nautilus_common::{
    live::sender::EventSender,
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_kalshi::{
    book::BookSyncTracker,
    common::{credential::KalshiCredential, enums::KalshiEnvironment},
    config::KalshiDataClientConfig,
    data::{quote, trade},
    execution::{PrivateState, fill_report, request},
    http::{
        client::{stable_object_key_bytes, validate_create},
        models::{
            BookSide, Event, Field, Fill, FixedPoint, Market, Order as VenueOrder, OrderStatus,
            OutcomeSide,
        },
        parse::parse_instrument,
    },
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        messages::{KalshiChannel, KalshiSubscribed, KalshiWsMessage},
        parse::decode_frame,
    },
};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    enums::{AccountType, OrderSide, OrderType, TimeInForce},
    events::{OrderAccepted, OrderEventAny, OrderSubmitted},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::BinaryOption,
    orders::{Order, OrderAny, builder::OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};

mod keys {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/keys.rs"
    ));
}

const CREATE_ORDER_PATH: &str = "/trade-api/v2/portfolio/events/orders";
const BOOK_SID: u64 = 7;
const GENERATION: u64 = 1;

fn ts_init() -> UnixNanos {
    UnixNanos::from(1_700_000_000_000_000_000)
}

pub(crate) fn live_instrument() -> BinaryOption {
    instrument_with_ticker(None)
}

fn book_instrument() -> BinaryOption {
    instrument_with_ticker(Some("MARKET-A"))
}

fn instrument_with_ticker(ticker: Option<&str>) -> BinaryOption {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
    let mut market: Market =
        serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();

    if let Some(ticker) = ticker {
        market.ticker = ticker.to_owned();
    }

    let event: Event =
        serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
    parse_instrument(&market, &event, ts_init()).unwrap()
}

pub(crate) fn quote_frame() -> Vec<u8> {
    data_frame("ticker", "ticker")
}

pub(crate) fn trade_frame() -> Vec<u8> {
    data_frame("trade", "trade")
}

fn data_frame(kind: &str, fixture: &str) -> Vec<u8> {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/market_data.json")).unwrap();
    json!({
        "type": kind,
        "sid": 12,
        "seq": 1,
        "msg": fixtures[fixture],
    })
    .to_string()
    .into_bytes()
}

pub(crate) fn decode_quote(frame: &[u8], instrument: &BinaryOption) -> Option<QuoteTick> {
    let KalshiWsMessage::Ticker(data) = decode_frame(black_box(frame)).unwrap() else {
        unreachable!("ticker frame")
    };

    quote(data.msg, instrument, ts_init()).unwrap()
}

pub(crate) fn decode_trade(frame: &[u8], instrument: &BinaryOption) -> Option<TradeTick> {
    let KalshiWsMessage::Trade(data) = decode_frame(black_box(frame)).unwrap() else {
        unreachable!("trade frame")
    };

    trade(data.msg, instrument, ts_init()).unwrap()
}

pub(crate) struct BookReady {
    tracker: BookSyncTracker,
    instruments: HashMap<InstrumentId, BinaryOption>,
    sender: EventSender<DataEvent>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    snapshot: Vec<u8>,
    delta: Vec<u8>,
}

impl BookReady {
    pub(crate) fn new() -> Self {
        let instrument = book_instrument();
        let id = instrument.id;
        let tracker = BookSyncTracker::new(&[id], std::time::Duration::from_secs(10));
        tracker.subscribe(id, None).unwrap();
        let (ws_client, _events) =
            KalshiWebSocketClient::new(KalshiDataClientConfig::default()).unwrap();
        tracker.replay(&ws_client).unwrap();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let instruments = HashMap::from([(id, instrument)]);
        let books: Value =
            serde_json::from_str(include_str!("../../test_data/books.json")).unwrap();

        let ready = Self {
            tracker,
            instruments,
            sender,
            receiver,
            snapshot: book_frame("orderbook_snapshot", &books["snapshot"], 1),
            delta: book_frame("orderbook_delta", &books["steps"][0], 2),
        };

        ready.apply(&subscribed());
        ready
    }

    pub(crate) fn snapshot_frame(&self) -> &[u8] {
        &self.snapshot
    }

    pub(crate) fn delta_frame(&self) -> &[u8] {
        &self.delta
    }

    pub(crate) fn apply_frame(&self, frame: &[u8]) {
        let message = decode_frame(black_box(frame)).unwrap();

        let event = KalshiWsEvent::Message {
            generation: GENERATION,
            message,
        };

        self.apply(&event);
    }

    fn apply(&self, event: &KalshiWsEvent) {
        self.tracker
            .event(event, &self.instruments, &self.sender, ts_init())
            .unwrap();
    }
}

fn book_frame(kind: &str, payload: &Value, seq: u64) -> Vec<u8> {
    json!({
        "type": kind,
        "sid": BOOK_SID,
        "seq": seq,
        "sending_ts_ms": 1_700_000_000_001u64,
        "msg": payload,
    })
    .to_string()
    .into_bytes()
}

fn subscribed() -> KalshiWsEvent {
    KalshiWsEvent::Message {
        generation: GENERATION,
        message: KalshiWsMessage::Subscribed {
            id: 1,
            msg: KalshiSubscribed {
                channel: KalshiChannel::OrderbookDelta,
                sid: BOOK_SID,
            },
        },
    }
}

pub(crate) fn credential(rsa: bool) -> KalshiCredential {
    let pem = if rsa {
        keys::rsa_pkcs8()
    } else {
        keys::ed25519_pkcs8()
    };

    KalshiCredential::resolve(
        Some("bench-key".into()),
        Some(pem.into()),
        KalshiEnvironment::Demo,
    )
    .unwrap()
}

pub(crate) fn limit_order(instrument: &BinaryOption) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-BENCH"))
        .instrument_id(instrument.id)
        .client_order_id(ClientOrderId::from("O-BENCH-1"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.25"))
        .price(Price::from("0.4200"))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

pub(crate) fn sign_submit(credential: &KalshiCredential, order: &OrderAny) -> (Vec<u8>, usize) {
    let built = request(black_box(order), None, 0, 0).unwrap();
    validate_create(&built).unwrap();
    let body = stable_object_key_bytes(serde_json::to_value(&built).unwrap()).unwrap();
    let headers = credential
        .headers(
            get_atomic_clock_realtime().get_time_ms(),
            "POST",
            CREATE_ORDER_PATH,
        )
        .unwrap();
    let count = headers.len();
    (black_box(body), black_box(count))
}

pub(crate) struct DispatchReady {
    state: PrivateState,
    source: VenueOrder,
    fill: Fill,
    instrument: BinaryOption,
    emitter: ExecutionEventEmitter,
    receiver: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
}

impl DispatchReady {
    pub(crate) fn tracked() -> Self {
        Self::new(true)
    }

    pub(crate) fn external() -> Self {
        Self::new(false)
    }

    fn new(tracked: bool) -> Self {
        let instrument = live_instrument();
        let source = venue_order(&instrument);
        let fill = venue_fill(&source);
        let (emitter, receiver) = emitter();
        let mut state = PrivateState::new();
        if tracked {
            state.restore(accepted_order(&instrument, &source, &emitter));
        }

        state
            .seed_positions([(instrument.id, Decimal::ZERO, ts_init())])
            .unwrap();

        Self {
            state,
            source,
            fill,
            instrument,
            emitter,
            receiver,
        }
    }

    pub(crate) fn dispatch(&mut self) {
        let report = fill_report(
            &self.fill,
            &self.instrument,
            self.emitter.account_id(),
            &self.source,
            ts_init(),
        )
        .unwrap();
        let plan = self
            .state
            .plan_order(
                &self.source,
                std::slice::from_ref(&report),
                &self.instrument,
                &self.emitter,
                ts_init(),
            )
            .unwrap()
            .unwrap();
        self.state.apply_plans(vec![plan], &self.emitter).unwrap();
        black_box(&self.receiver);
    }
}

fn emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let mut emitter = ExecutionEventEmitter::new(
        get_atomic_clock_realtime(),
        TraderId::from("TRADER-001"),
        AccountId::from("KALSHI-007"),
        AccountType::Cash,
        Some(Currency::USD()),
    );

    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(sender);
    (emitter, receiver)
}

fn venue_order(instrument: &BinaryOption) -> VenueOrder {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let mut source: VenueOrder =
        serde_json::from_value(fixtures["order"]["order"].clone()).unwrap();
    source.ticker = instrument.id.symbol.to_string();
    source.client_order_id = "client-a".to_owned();
    source.book_side = BookSide::Bid;
    source.outcome_side = OutcomeSide::Yes;
    source.yes_price_dollars = FixedPoint(Decimal::new(6001, 4));
    source.no_price_dollars = FixedPoint(Decimal::new(3999, 4));
    source.fill_count_fp = FixedPoint(Decimal::new(75, 2));
    source.remaining_count_fp = FixedPoint(Decimal::new(125, 2));
    source.initial_count_fp = FixedPoint(Decimal::from(2));
    source.expiration_time = Field::Null;
    source.status = OrderStatus::Resting;
    source
}

fn venue_fill(source: &VenueOrder) -> Fill {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    let mut fill: Fill = serde_json::from_value(fixtures["fills"]["fills"][0].clone()).unwrap();
    fill.order_id = source.order_id.clone();
    fill.ticker = source.ticker.clone();
    fill.market_ticker = source.ticker.clone();
    fill.book_side = source.book_side;
    fill.outcome_side = source.outcome_side;
    fill.yes_price_dollars = source.yes_price_dollars;
    fill.no_price_dollars = source.no_price_dollars;
    fill.count_fp = source.fill_count_fp;
    fill.created_time = source.created_time.clone();
    fill
}

fn accepted_order(
    instrument: &BinaryOption,
    source: &VenueOrder,
    emitter: &ExecutionEventEmitter,
) -> OrderAny {
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-BENCH"))
        .instrument_id(instrument.id)
        .client_order_id(ClientOrderId::from(source.client_order_id.as_str()))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("2.00"))
        .price(Price::from("0.600100"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        instrument.id,
        order.client_order_id(),
        emitter.account_id(),
        UUID4::new(),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        instrument.id,
        order.client_order_id(),
        VenueOrderId::from(source.order_id.as_str()),
        emitter.account_id(),
        UUID4::new(),
        UnixNanos::from(3),
        UnixNanos::from(4),
        false,
    );

    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    order
}
