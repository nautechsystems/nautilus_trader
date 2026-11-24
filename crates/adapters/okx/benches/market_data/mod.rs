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

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

use criterion::{Criterion, Throughput};
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    enums::AggressorSide,
    identifiers::{Symbol, TradeId},
    instruments::stubs::currency_pair_btcusdt,
    types::{Price, Quantity},
};
use rstest::rstest;
use serde_json::{Value, json};

use super::*;
use crate::{
    common::enums::OKXEnvironment,
    websocket::{enums::OKXWsOperation, messages::OKXWsFrame},
};

const BBO: &str = include_str!("../../test_data/ws_bbo_tbt.json");
const PARTIAL_BBO: &str = include_str!("../../test_data/ws_bbo_tbt_partial.json");
const TRADE: &str = include_str!("../../test_data/ws_trades.json");
const INDEX: &str = include_str!("../../test_data/ws_index_tickers.json");
const INDEX_SYMBOLS: [&str; 5] = [
    "BTC-USDT-SWAP",
    "BTC-USDT-241227",
    "BTC-USDT-240628",
    "BTC-USDT-250328",
    "BTC-USDT-240906",
];

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[allow(
    unsafe_code,
    reason = "the benchmark counts allocations through the system allocator"
)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: The caller provides a valid layout
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: The caller provides a valid layout
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller provides a pointer and layout from this allocator
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: The caller provides a matching allocation and valid new size
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}

#[rstest]
#[ignore = "benchmark, run separately with --profile bench-lto"]
fn public_market_data_benchmark() {
    let mut pipeline = MarketDataPipeline::new();
    pipeline.handle(BBO);
    assert!(matches!(
        pipeline.receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Quote(_))
    ));

    let mut criterion = Criterion::default()
        .without_plots()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    let mut group = criterion.benchmark_group("public_market_data");
    group.throughput(Throughput::Elements(1));

    for (name, frame, expected) in [
        ("partial_bbo", PARTIAL_BBO, 1),
        ("trades", TRADE, 1),
        ("index_fanout", INDEX, INDEX_SYMBOLS.len()),
    ] {
        const FRAMES: u32 = 1024;
        pipeline.handle(frame);
        for _ in 0..expected {
            pipeline.receiver.try_recv().unwrap();
        }

        assert!(pipeline.receiver.try_recv().is_err());

        let mut dispatch = || {
            pipeline.handle(black_box(frame));
            for _ in 0..expected {
                black_box(pipeline.receiver.try_recv().unwrap());
            }
        };

        for _ in 0..FRAMES {
            dispatch();
        }

        ALLOCATIONS.set(Some(0));

        for _ in 0..FRAMES {
            dispatch();
        }

        let count = u32::try_from(ALLOCATIONS.replace(None).unwrap()).unwrap();
        let calls_per_frame = f64::from(count) / f64::from(FRAMES);
        println!("alloc {name} calls/frame={calls_per_frame:.3}");
        group.bench_function(name, |b| b.iter(&mut dispatch));
    }

    group.finish();
    criterion.final_summary();
}

#[rstest]
#[case::bbo(BBO, OKXWsChannel::BboTbt, "BTC-USDT")]
#[case::trades(TRADE, OKXWsChannel::Trades, "BTC-USD")]
fn public_market_data_wire_paths(
    #[case] frame: &str,
    #[case] channel: OKXWsChannel,
    #[case] symbol: &str,
) {
    let expected: Value = serde_json::from_str(frame).unwrap();

    let OKXWsInbound::MarketData { inst_id, data } = OKXWsInbound::decode(frame, true).unwrap()
    else {
        panic!("Expected direct market data for {channel:?}");
    };

    assert_eq!(inst_id, Some(Ustr::from(symbol)));
    assert_eq!(data.channel(), channel);

    for frame in [
        OKXWsInbound::decode(&data_first(&expected), true)
            .unwrap()
            .into_standard()
            .unwrap(),
        serde_json::from_str(frame).unwrap(),
    ] {
        let OKXWsFrame::Data { arg, data } = frame else {
            panic!("Expected generic data for {channel:?}");
        };

        assert_eq!(arg.channel, channel);
        assert_eq!(arg.inst_id, Some(Ustr::from(symbol)));
        assert_eq!(data, expected["data"]);
    }
}

#[rstest]
fn public_market_data_defers_to_late_control_fields() {
    let bbo: Value = serde_json::from_str(BBO).unwrap();
    let order = json!({
        "arg": bbo["arg"],
        "data": bbo["data"],
        "op": "order",
        "code": "0",
        "msg": "",
    })
    .to_string();
    let error = json!({
        "arg": bbo["arg"],
        "data": {"unexpected": true},
        "event": "error",
        "code": "60012",
        "msg": "Invalid request",
    })
    .to_string();

    assert!(matches!(
        OKXWsInbound::decode(&order, true).unwrap(),
        OKXWsInbound::Standard(OKXWsFrame::OrderResponse { op: OKXWsOperation::Order, code, msg, data, .. })
            if code == "0" && msg.is_empty() && data == *bbo["data"].as_array().unwrap()
    ));
    assert!(matches!(
        OKXWsInbound::decode(&error, true).unwrap(),
        OKXWsInbound::Standard(OKXWsFrame::Error { arg: Some(arg), code, msg })
            if arg.channel == OKXWsChannel::BboTbt && code == "60012" && msg == "Invalid request"
    ));
}

#[rstest]
#[case::direct(false)]
#[case::fallback(true)]
fn public_market_data_merges_partial_bbo(#[case] reordered: bool) {
    let mut pipeline = MarketDataPipeline::new();
    let partial = in_wire_order(PARTIAL_BBO, reordered);
    pipeline.handle(&partial);
    assert!(pipeline.receiver.try_recv().is_err());
    assert!(pipeline.quote_cache.is_empty());

    pipeline.handle(BBO);
    let initial = pipeline.receiver.try_recv().unwrap();
    pipeline.handle(&partial);
    let updated = pipeline.receiver.try_recv().unwrap();

    let instrument_id = InstrumentId::from("BTC-USDT.OKX");

    let initial_quote = QuoteTick::new(
        instrument_id,
        Price::from("8476.97"),
        Price::from("8476.98"),
        Quantity::from("256.00000000"),
        Quantity::from("415.00000000"),
        UnixNanos::from(1_597_026_383_085_000_000),
        UnixNanos::from(42),
    );

    let updated_quote = QuoteTick::new(
        instrument_id,
        Price::from("8476.97"),
        Price::from("8476.99"),
        Quantity::from("256.00000000"),
        Quantity::from("416.00000001"),
        UnixNanos::from(1_597_026_383_086_000_000),
        UnixNanos::from(42),
    );

    for (event, expected) in [(initial, initial_quote), (updated, updated_quote)] {
        let DataEvent::Data(Data::Quote(quote)) = event else {
            panic!("Expected a quote tick, received {event:?}");
        };

        assert_eq!(quote, expected);
        assert_eq!(
            (
                quote.bid_price.precision,
                quote.ask_price.precision,
                quote.bid_size.precision,
                quote.ask_size.precision,
            ),
            (2, 2, 8, 8),
        );
    }

    assert!(pipeline.receiver.try_recv().is_err());
}

#[rstest]
#[case::direct(false)]
#[case::fallback(true)]
fn public_market_data_preserves_trade_batch_order(#[case] reordered: bool) {
    let mut frame: Value = serde_json::from_str(TRADE).unwrap();
    let mut second = frame["data"][0].clone();
    second["tradeId"] = json!("130639475");
    second["px"] = json!("42220.1");
    second["sz"] = json!("0.02000001");
    second["side"] = json!("sell");
    second["ts"] = json!("1630048897898");
    frame["data"].as_array_mut().unwrap().push(second);

    let mut pipeline = MarketDataPipeline::new();
    pipeline.handle(&in_wire_order(&frame.to_string(), reordered));

    let instrument_id = InstrumentId::from("BTC-USD.OKX");

    for expected in [
        TradeTick::new(
            instrument_id,
            Price::from("42219.9"),
            Quantity::from("0.12060306"),
            AggressorSide::Buy,
            TradeId::new("130639474"),
            UnixNanos::from(1_630_048_897_897_000_000),
            UnixNanos::from(42),
        ),
        TradeTick::new(
            instrument_id,
            Price::from("42220.1"),
            Quantity::from("0.02000001"),
            AggressorSide::Sell,
            TradeId::new("130639475"),
            UnixNanos::from(1_630_048_897_898_000_000),
            UnixNanos::from(42),
        ),
    ] {
        let event = pipeline.receiver.try_recv().unwrap();

        let DataEvent::Data(Data::Trade(trade)) = event else {
            panic!("Expected a trade tick, received {event:?}");
        };

        assert_eq!(trade, expected);
        assert_eq!((trade.price.precision, trade.size.precision), (1, 8));
    }

    assert!(pipeline.receiver.try_recv().is_err());

    frame["data"][1]["sz"] = json!("invalid");
    pipeline.handle(&in_wire_order(&frame.to_string(), reordered));
    assert!(
        pipeline.receiver.try_recv().is_err(),
        "a failed trade must suppress the whole batch"
    );
}

#[rstest]
fn public_market_data_index_prices_use_target_precision_and_batch_order() {
    let mut frame: Value = serde_json::from_str(INDEX).unwrap();
    let mut second = frame["data"][0].clone();
    second["idxPx"] = json!("42220.187");
    second["ts"] = json!("1597026383086");
    frame["data"].as_array_mut().unwrap().push(second);

    let mut pipeline = MarketDataPipeline::new();
    pipeline.handle(&frame.to_string());

    for (symbol, prices) in [
        ("BTC-USDT-240628.OKX", ["42220.123", "42220.187"]),
        ("BTC-USDT-240906.OKX", ["42220.12", "42220.19"]),
        ("BTC-USDT-241227.OKX", ["42220.12", "42220.19"]),
        ("BTC-USDT-250328.OKX", ["42220.12", "42220.19"]),
        ("BTC-USDT-SWAP.OKX", ["42220.1", "42220.2"]),
    ] {
        for (price, ts_event) in prices
            .into_iter()
            .zip([1_597_026_383_085_000_000, 1_597_026_383_086_000_000])
        {
            let event = pipeline.receiver.try_recv().unwrap();

            let DataEvent::Data(Data::IndexPrice(update)) = event else {
                panic!("Expected index price, received {event:?}");
            };

            let expected = Price::from(price);
            assert_eq!(update.instrument_id, InstrumentId::from(symbol));
            assert_eq!(update.value, expected);
            assert_eq!(update.value.precision, expected.precision);
            assert_eq!(update.ts_event, UnixNanos::from(ts_event));
            assert_eq!(update.ts_init, UnixNanos::from(42));
        }
    }

    assert!(pipeline.receiver.try_recv().is_err());

    frame["data"][1]["idxPx"] = json!("invalid");
    pipeline.handle(&frame.to_string());
    assert!(
        pipeline.receiver.try_recv().is_err(),
        "a failed index price must suppress the whole batch"
    );
}

fn in_wire_order(raw: &str, reordered: bool) -> String {
    if reordered {
        data_first(&serde_json::from_str(raw).unwrap())
    } else {
        raw.to_owned()
    }
}

fn data_first(frame: &Value) -> String {
    format!(r#"{{"data":{},"arg":{}}}"#, frame["data"], frame["arg"])
}

struct MarketDataPipeline {
    clock: AtomicTime,
    http: OKXHttpClient,
    config: OKXDataClientConfig,
    update_lock: parking_lot::Mutex<()>,
    instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    book_channels: Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    book_sync: BookSyncTracker,
    book_depths: AtomicMap<InstrumentId, usize>,
    book_deltas: AtomicSet<InstrumentId>,
    quote_cache: QuoteCache,
    funding_cache: AHashMap<Ustr, (Ustr, u64)>,
    index_ticker_map: Arc<AtomicMap<Ustr, AHashSet<Ustr>>>,
    option_greeks_subs: Arc<AtomicMap<InstrumentId, AHashSet<OKXGreeksType>>>,
    _task_group: TaskGroup,
    tasks: TaskSpawner,
    sender: EventSender<DataEvent>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
}

impl MarketDataPipeline {
    fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());

        for symbol in ["BTC-USDT", "BTC-USD"].into_iter().chain(INDEX_SYMBOLS) {
            let mut pair = currency_pair_btcusdt();
            pair.id = InstrumentId::new(Symbol::from(symbol), *OKX_VENUE);
            pair.raw_symbol = pair.id.symbol;
            pair.size_increment = Quantity::from("0.00000001");
            pair.size_precision = pair.size_increment.precision;

            pair.price_increment = match symbol {
                "BTC-USD" | "BTC-USDT-SWAP" => Price::from("0.1"),
                "BTC-USDT-240628" => Price::from("0.001"),
                _ => Price::from("0.01"),
            };

            pair.price_precision = pair.price_increment.precision;
            instruments.insert(Ustr::from(symbol), InstrumentAny::CurrencyPair(pair));
        }

        let index_ticker_map = Arc::new(AtomicMap::new());
        index_ticker_map.insert(
            Ustr::from("BTC-USDT"),
            INDEX_SYMBOLS.map(Ustr::from).into_iter().collect(),
        );
        let task_group = TaskGroup::new();
        let tasks = task_group.spawner().unwrap();

        Self {
            clock: AtomicTime::new(false, UnixNanos::from(42)),
            http: OKXHttpClient::new(
                Some("http://127.0.0.1:9".into()),
                5,
                0,
                1,
                1,
                OKXEnvironment::Live,
                None,
            )
            .unwrap(),
            config: OKXDataClientConfig::default(),
            update_lock: parking_lot::Mutex::default(),
            instruments,
            book_channels: Arc::new(AtomicMap::new()),
            book_sync: BookSyncTracker::default(),
            book_depths: AtomicMap::new(),
            book_deltas: AtomicSet::new(),
            quote_cache: QuoteCache::new(),
            funding_cache: AHashMap::new(),
            index_ticker_map,
            option_greeks_subs: Arc::new(AtomicMap::new()),
            _task_group: task_group,
            tasks,
            sender: sender.into(),
            receiver,
        }
    }

    fn handle(&mut self, raw: &str) {
        let message = match OKXWsInbound::decode(raw, true).unwrap() {
            OKXWsInbound::Standard(OKXWsFrame::Data { arg, data }) => OKXWsMessage::ChannelData {
                channel: arg.channel,
                inst_id: arg.inst_id,
                data,
            }
            .into(),
            OKXWsInbound::MarketData { inst_id, data } => {
                OKXWsInbound::MarketData { inst_id, data }
            }
            frame => panic!("Expected public market data frame, received {frame:?}"),
        };

        OKXDataClient::handle_ws_message(
            message,
            &self.sender,
            &self.instruments,
            &self.http,
            &self.config,
            &self.update_lock,
            &self.book_channels,
            &self.book_sync,
            &self.book_depths,
            &self.book_deltas,
            None,
            None,
            &mut self.quote_cache,
            &mut self.funding_cache,
            &self.index_ticker_map,
            &self.option_greeks_subs,
            BookChannelScope::Public,
            Duration::ZERO,
            &self.tasks,
            &self.clock,
        );
    }
}
