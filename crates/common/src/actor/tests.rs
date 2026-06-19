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
    any::Any,
    cell::{RefCell, UnsafeCell},
    num::NonZeroUsize,
    rc::Rc,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use ahash::AHashSet;
use bytes::Bytes;
use indexmap::IndexMap;
use log::LevelFilter;
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, BarType, BookOrder, CustomData, DataType, FundingRateUpdate, GreeksData, HasTsInit,
        IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
        close::InstrumentClose,
        custom::CustomDataTrait,
        greeks::OptionGreekValues,
        option_chain::{OptionChainSlice, OptionGreeks, StrikeRange},
        stubs::*,
    },
    enums::{
        BookAction, BookType, GreeksConvention, OrderSide, OrderType, PositionSide, PriceType,
    },
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
        OptionSeriesId, PositionId, StrategyId, Symbol, TraderId, Venue, VenueOrderId,
    },
    instruments::{CurrencyPair, Instrument, InstrumentAny, SyntheticInstrument, stubs::*},
    orderbook::{OrderBook, own::OwnOrderBook},
    orders::{Order, OrderAny, OrderList, builder::OrderTestBuilder},
    position::Position,
    stubs::TestDefault,
    types::{Currency, Price, Quantity},
};
use rstest::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use ustr::Ustr;
#[cfg(feature = "defi")]
use {
    alloy_primitives::{Address, I256, U160},
    nautilus_model::defi::{
        Block, Blockchain, Dex, DexType, Pool, PoolIdentifier, PoolLiquidityUpdate, PoolProfiler,
        PoolSwap, Token, chain::chains, dex::AmmType,
    },
};

use super::{Actor, DataActor, DataActorCore, data_actor::DataActorConfig};
#[cfg(feature = "defi")]
use crate::defi::switchboard::{
    get_defi_blocks_topic, get_defi_pool_swaps_topic, get_defi_pool_topic,
};
use crate::{
    actor::registry::{get_actor, get_actor_unchecked, register_actor},
    cache::Cache,
    clock::TestClock,
    component::Component,
    logging::{logger::LogGuard, logging_is_initialized},
    messages::data::{
        BarsResponse, BookDeltasResponse, BookDepthResponse, BookResponse, CustomDataResponse,
        DataResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
        PARAMS_IS_PARENT, QuotesResponse, TradesResponse,
    },
    msgbus::{
        self, MessageBus, get_message_bus,
        switchboard::{
            MessagingSwitchboard, get_bars_topic, get_book_deltas_topic, get_book_snapshots_topic,
            get_custom_topic, get_funding_rate_topic, get_index_price_topic,
            get_instrument_close_topic, get_instrument_status_topic, get_instrument_topic,
            get_mark_price_topic, get_option_chain_topic, get_option_greeks_topic,
            get_quotes_topic, get_trades_topic,
        },
    },
    nautilus_actor,
    runner::{SyncDataCommandSender, set_data_cmd_sender},
    signal::Signal,
    testing::init_logger_for_testing,
    timer::TimeEvent,
};

/// Minimal custom data type for actor tests.
#[derive(Clone, Debug, PartialEq, Serialize)]
struct TestActorCustomData {
    label: String,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl HasTsInit for TestActorCustomData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for TestActorCustomData {
    fn type_name(&self) -> &'static str {
        "TestActorCustomData"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }
    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }
    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(other) = other.as_any().downcast_ref::<Self>() {
            self == other
        } else {
            false
        }
    }
}

pub(crate) fn make_test_custom_data(label: &str) -> CustomData {
    CustomData::from_arc(Arc::new(TestActorCustomData {
        label: label.to_string(),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    }))
}

#[derive(Debug)]
struct TestDataActor {
    core: DataActorCore,
    pub received_time_events: Vec<TimeEvent>,
    pub received_instruments: Vec<InstrumentAny>,
    pub received_data: Vec<String>, // Use string for simplicity
    pub received_books: Vec<OrderBook>,
    pub received_deltas: Vec<OrderBookDelta>,
    pub received_depths: Vec<OrderBookDepth10>,
    pub received_quotes: Vec<QuoteTick>,
    pub received_trades: Vec<TradeTick>,
    pub received_bars: Vec<Bar>,
    pub received_mark_prices: Vec<MarkPriceUpdate>,
    pub received_index_prices: Vec<IndexPriceUpdate>,
    pub received_funding_rates: Vec<FundingRateUpdate>,
    pub received_status: Vec<InstrumentStatus>,
    pub received_closes: Vec<InstrumentClose>,
    pub received_greeks: Vec<OptionGreeks>,
    pub received_chain_slices: Vec<OptionChainSlice>,
    pub received_signals: Vec<Signal>,
    pub received_custom_data: Vec<CustomData>,
    #[cfg(feature = "defi")]
    pub received_blocks: Vec<Block>,
    #[cfg(feature = "defi")]
    pub received_pools: Vec<Pool>,
    #[cfg(feature = "defi")]
    pub received_pool_swaps: Vec<PoolSwap>,
    #[cfg(feature = "defi")]
    pub received_pool_liquidity_updates: Vec<PoolLiquidityUpdate>,
}

nautilus_actor!(TestDataActor);

impl DataActor for TestDataActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting actor"); // Custom log
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        self.received_time_events.push(event.clone());
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        self.received_instruments.push(instrument.clone());
        Ok(())
    }

    fn on_data(&mut self, data: &CustomData) -> anyhow::Result<()> {
        self.received_data.push(data.data_type.to_string());
        self.received_custom_data.push(data.clone());
        Ok(())
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        self.received_signals.push(signal.clone());
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.received_deltas.extend(&deltas.deltas);
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        self.received_books.push(book.clone());
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.received_quotes.push(*quote);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        self.received_trades.push(*trade);
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.received_bars.push(*bar);
        Ok(())
    }

    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_quotes.extend(quotes);
        Ok(())
    }

    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_trades.extend(trades);
        Ok(())
    }

    fn on_historical_book_deltas(&mut self, deltas: &[OrderBookDelta]) -> anyhow::Result<()> {
        self.received_deltas.extend(deltas);
        Ok(())
    }

    fn on_historical_book_depth(&mut self, depths: &[OrderBookDepth10]) -> anyhow::Result<()> {
        self.received_depths.extend(depths);
        Ok(())
    }

    fn on_historical_funding_rates(
        &mut self,
        funding_rates: &[FundingRateUpdate],
    ) -> anyhow::Result<()> {
        self.received_funding_rates.extend(funding_rates);
        Ok(())
    }

    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_bars.extend(bars);
        Ok(())
    }

    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        self.received_data.push(format!("{data:?}"));
        Ok(())
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        self.received_mark_prices.push(*mark_price);
        Ok(())
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        self.received_index_prices.push(*index_price);
        Ok(())
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        self.received_funding_rates.push(*funding_rate);
        Ok(())
    }

    fn on_instrument_status(&mut self, status: &InstrumentStatus) -> anyhow::Result<()> {
        self.received_status.push(*status);
        Ok(())
    }

    fn on_instrument_close(&mut self, close: &InstrumentClose) -> anyhow::Result<()> {
        self.received_closes.push(*close);
        Ok(())
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        self.received_greeks.push(*greeks);
        Ok(())
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        self.received_chain_slices.push(slice.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
        self.received_blocks.push(block.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
        self.received_pools.push(pool.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        self.received_pool_swaps.push(swap.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        self.received_pool_liquidity_updates.push(update.clone());
        Ok(())
    }
}

// Custom functionality as required
impl TestDataActor {
    pub(crate) fn new(config: DataActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config),
            received_time_events: Vec::new(),
            received_instruments: Vec::new(),
            received_data: Vec::new(),
            received_books: Vec::new(),
            received_deltas: Vec::new(),
            received_depths: Vec::new(),
            received_quotes: Vec::new(),
            received_trades: Vec::new(),
            received_bars: Vec::new(),
            received_mark_prices: Vec::new(),
            received_index_prices: Vec::new(),
            received_funding_rates: Vec::new(),
            received_status: Vec::new(),
            received_closes: Vec::new(),
            received_greeks: Vec::new(),
            received_chain_slices: Vec::new(),
            received_signals: Vec::new(),
            received_custom_data: Vec::new(),
            #[cfg(feature = "defi")]
            received_blocks: Vec::new(),
            #[cfg(feature = "defi")]
            received_pools: Vec::new(),
            #[cfg(feature = "defi")]
            received_pool_swaps: Vec::new(),
            #[cfg(feature = "defi")]
            received_pool_liquidity_updates: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn custom_function(&self) {}
}

#[fixture]
pub fn clock() -> Rc<RefCell<TestClock>> {
    Rc::new(RefCell::new(TestClock::new()))
}

#[fixture]
pub fn cache() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::new(None, None)))
}

#[fixture]
fn switchboard() -> Arc<MessagingSwitchboard> {
    Arc::new(MessagingSwitchboard::default())
}

#[fixture]
fn trader_id() -> TraderId {
    TraderId::test_default()
}

#[fixture]
fn test_logging() -> Option<LogGuard> {
    // Avoid reinitializing logger if already set
    if logging_is_initialized() {
        return None;
    }

    Some(init_logger_for_testing(Some(LevelFilter::Trace)).unwrap())
}

/// A simple Actor implementation for testing.
#[derive(Debug)]
struct DummyActor {
    id_str: Ustr,
    count: usize,
}
impl DummyActor {
    fn new<S: AsRef<str>>(s: S) -> Self {
        Self {
            id_str: Ustr::from_str(s.as_ref()).unwrap(),
            count: 0,
        }
    }
}
impl Actor for DummyActor {
    fn id(&self) -> Ustr {
        self.id_str
    }
    fn handle(&mut self, _msg: &dyn std::any::Any) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn register_data_actor(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) -> Ustr {
    // Set up sync data command sender for tests
    set_data_cmd_sender(Arc::new(SyncDataCommandSender));

    let config = DataActorConfig::default();
    // Ensure clean message bus state for this actor's subscriptions
    let bus = get_message_bus();
    *bus.borrow_mut() = MessageBus::default();
    let mut actor = TestDataActor::new(config);
    actor.register(trader_id, clock, cache).unwrap();

    let actor_id = actor.actor_id();

    register_actor(actor);
    actor_id.inner()
}

#[rstest]
fn test_data_actor_clock_api(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache).unwrap();

    actor
        .clock()
        .set_timer(
            "TEST-TIMER",
            Duration::from_secs(1),
            None,
            None,
            None,
            Some(true),
            Some(false),
        )
        .unwrap();

    assert_eq!(actor.clock().timestamp_ns(), UnixNanos::default());
    assert_eq!(actor.clock().timestamp_us(), 0);
    assert_eq!(actor.clock().timestamp_ms(), 0);
    assert_eq!(actor.clock().timestamp(), 0.0);
    assert_eq!(actor.clock().timer_count(), 1);
    assert_eq!(actor.clock().timer_names(), vec!["TEST-TIMER".to_string()]);
    assert!(actor.clock().timer_exists("TEST-TIMER"));
    assert!(!actor.clock().timer_exists("MISSING-TIMER"));
    assert_eq!(
        actor.clock().next_time_ns("TEST-TIMER"),
        Some(UnixNanos::from(1_000_000_000_u64))
    );

    actor.clock().cancel_timer("TEST-TIMER");

    assert_eq!(actor.clock().timer_count(), 0);
    assert_eq!(actor.clock().timer_names(), Vec::<String>::new());
    assert!(!actor.clock().timer_exists("TEST-TIMER"));

    actor
        .clock()
        .set_timer_ns(
            "TEST-TIMER-NS",
            2_000_000_000,
            None,
            None,
            None,
            Some(true),
            Some(false),
        )
        .unwrap();

    assert_eq!(
        actor.clock().next_time_ns("TEST-TIMER-NS"),
        Some(UnixNanos::from(2_000_000_000_u64))
    );

    actor.clock().cancel_timers();

    assert_eq!(actor.clock().timer_count(), 0);
    assert_eq!(actor.clock().timer_names(), Vec::<String>::new());
}

#[rstest]
fn test_data_actor_cache_api_returns_owned_point_reads(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache.clone()).unwrap();

    let instrument_id = audusd_sim.id;
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();
    let client_order_id = order.client_order_id();

    {
        let mut cache = cache.borrow_mut();
        cache.add_instrument(instrument.clone()).unwrap();
        cache.add_order(order, None, None, false).unwrap();
    }

    let cache_api = actor.cache();
    let cached_instrument = cache_api.try_instrument(&instrument_id).unwrap();
    let maybe_instrument = cache_api.instrument(&instrument_id);
    let cached_order = cache_api.try_order(&client_order_id).unwrap();
    let maybe_order = cache_api.order(&client_order_id).unwrap();
    let missing_instrument_id = InstrumentId::from("MISSING.SIM");
    let missing_instrument = cache_api
        .try_instrument(&missing_instrument_id)
        .unwrap_err();
    let missing_order_id = ClientOrderId::from("O-MISSING");
    let missing_order = cache_api.try_order(&missing_order_id).unwrap_err();

    let _cache_write = cache.borrow_mut();

    assert_eq!(cached_instrument, instrument);
    assert_eq!(maybe_instrument, Some(instrument));
    assert_eq!(cached_order.client_order_id(), client_order_id);
    assert_eq!(maybe_order.client_order_id(), client_order_id);
    assert_eq!(
        missing_instrument,
        crate::cache::InstrumentLookupError::not_found(missing_instrument_id)
    );
    assert_eq!(
        missing_order,
        crate::cache::OrderLookupError::not_found(missing_order_id)
    );
}

#[rstest]
fn test_data_actor_cache_api_returns_owned_market_data_point_reads(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    quote_audusd: QuoteTick,
    stub_bar: Bar,
    stub_instrument_status: InstrumentStatus,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache.clone()).unwrap();

    let instrument_id = audusd_sim.id;
    let venue = instrument_id.venue;
    let quote = QuoteTick {
        instrument_id,
        ..quote_audusd
    };
    let trade = TradeTick {
        instrument_id,
        price: Price::from("1.00020"),
        ..TradeTick::default()
    };
    let bar = stub_bar;
    let bar_type = bar.bar_type;
    let mark_price = MarkPriceUpdate::new(
        instrument_id,
        Price::from("1.00030"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let index_price = IndexPriceUpdate::new(
        instrument_id,
        Price::from("1.00040"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    let funding_rate = FundingRateUpdate::new(
        instrument_id,
        dec!(0.0001),
        None,
        None,
        UnixNanos::from(5),
        UnixNanos::from(6),
    );
    let status = InstrumentStatus {
        instrument_id,
        ..stub_instrument_status
    };
    let greeks = GreeksData::new(
        UnixNanos::from(7),
        UnixNanos::from(8),
        instrument_id,
        true,
        1.0,
        20_260_101,
        30,
        30.0 / 365.0,
        100_000.0,
        1.0,
        1.00020,
        0.05,
        0.0,
        0.2,
        12.5,
        0.01,
        OptionGreekValues {
            delta: 0.5,
            gamma: 0.1,
            vega: 0.2,
            theta: -0.01,
            rho: 0.03,
        },
        0.6,
    );
    let option_greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.55,
            gamma: 0.03,
            vega: 0.12,
            theta: -0.05,
            rho: 0.01,
        },
        mark_iv: Some(0.25),
        bid_iv: Some(0.24),
        ask_iv: Some(0.26),
        underlying_price: Some(1.00020),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::from(9),
        ts_init: UnixNanos::from(10),
    };
    let synthetic_formula = format!("{instrument_id} * 1.0");
    let synthetic = SyntheticInstrument::new(
        Symbol::from("SYN"),
        5,
        vec![instrument_id],
        &synthetic_formula,
        UnixNanos::from(11),
        UnixNanos::from(12),
    );
    let synthetic_id = synthetic.id;
    let usd = Currency::USD();
    let usdt = Currency::USDT();

    {
        let mut cache = cache.borrow_mut();
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
        book.update_quote_tick(&quote).unwrap();

        cache
            .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
            .unwrap();
        cache.add_order_book(book).unwrap();
        cache.add_quote(quote).unwrap();
        cache.add_trade(trade).unwrap();
        cache.add_bar(bar).unwrap();
        cache.add_mark_price(mark_price).unwrap();
        cache.add_index_price(index_price).unwrap();
        cache.add_funding_rate(funding_rate).unwrap();
        cache.add_instrument_status(status).unwrap();
        cache.add_greeks(greeks.clone()).unwrap();
        cache.add_option_greeks(option_greeks);
        cache.add_synthetic(synthetic).unwrap();
        cache.add_currency(usd).unwrap();
        cache.set_mark_xrate(usd, usdt, 1.1);
    }

    let cache_api = actor.cache();
    let cached_synthetic = cache_api.synthetic(&synthetic_id).unwrap();
    let bid_price = cache_api.price(&instrument_id, PriceType::Bid);
    let latest_quote = cache_api.quote(&instrument_id);
    let indexed_quote = cache_api.quote_at_index(&instrument_id, 0);
    let latest_trade = cache_api.trade(&instrument_id);
    let indexed_trade = cache_api.trade_at_index(&instrument_id, 0);
    let latest_bar = cache_api.bar(&bar_type);
    let indexed_bar = cache_api.bar_at_index(&bar_type, 0);
    let latest_mark_price = cache_api.mark_price(&instrument_id);
    let latest_index_price = cache_api.index_price(&instrument_id);
    let latest_funding_rate = cache_api.funding_rate(&instrument_id);
    let latest_status = cache_api.instrument_status(&instrument_id);
    let book_update_count = cache_api.book_update_count(&instrument_id);
    let quote_count = cache_api.quote_count(&instrument_id);
    let trade_count = cache_api.trade_count(&instrument_id);
    let bar_count = cache_api.bar_count(&bar_type);
    let has_order_book = cache_api.has_order_book(&instrument_id);
    let has_quote_ticks = cache_api.has_quote_ticks(&instrument_id);
    let has_trade_ticks = cache_api.has_trade_ticks(&instrument_id);
    let has_bars = cache_api.has_bars(&bar_type);
    let cached_currency = cache_api.currency(&usd.code);
    let required_currency = cache_api.try_currency(&usd.code).unwrap();
    let same_currency_xrate = cache_api.get_xrate(venue, usd, usd, PriceType::Mid);
    let mark_xrate = cache_api.get_mark_xrate(usd, usdt);
    let cached_greeks = cache_api.greeks(&instrument_id).unwrap();
    let cached_option_greeks = cache_api.option_greeks(&instrument_id);
    let quote_out_of_range = cache_api.quote_at_index(&instrument_id, 1);
    let trade_out_of_range = cache_api.trade_at_index(&instrument_id, 1);
    let bar_out_of_range = cache_api.bar_at_index(&bar_type, 1);
    let missing_currency_code = Ustr::from("ZZZ");
    let missing_currency = cache_api.currency(&missing_currency_code);
    let missing_currency_error = cache_api.try_currency(&missing_currency_code).unwrap_err();

    let _cache_write = cache.borrow_mut();

    assert_eq!(cached_synthetic.id, synthetic_id);
    assert_eq!(bid_price, Some(quote.bid_price));
    assert_eq!(latest_quote, Some(quote));
    assert_eq!(indexed_quote, Some(quote));
    assert_eq!(latest_trade, Some(trade));
    assert_eq!(indexed_trade, Some(trade));
    assert_eq!(latest_bar, Some(bar));
    assert_eq!(indexed_bar, Some(bar));
    assert_eq!(latest_mark_price, Some(mark_price));
    assert_eq!(latest_index_price, Some(index_price));
    assert_eq!(latest_funding_rate, Some(funding_rate));
    assert_eq!(latest_status, Some(status));
    assert_eq!(book_update_count, 1);
    assert_eq!(quote_count, 1);
    assert_eq!(trade_count, 1);
    assert_eq!(bar_count, 1);
    assert!(has_order_book);
    assert!(has_quote_ticks);
    assert!(has_trade_ticks);
    assert!(has_bars);
    assert_eq!(cached_currency, Some(usd));
    assert_eq!(required_currency, usd);
    assert_eq!(same_currency_xrate, Some(Decimal::ONE));
    assert_eq!(mark_xrate, Some(1.1));
    assert_eq!(cached_greeks.instrument_id, instrument_id);
    assert_eq!(cached_greeks.price, greeks.price);
    assert_eq!(cached_option_greeks, Some(option_greeks));
    assert_eq!(quote_out_of_range, None);
    assert_eq!(trade_out_of_range, None);
    assert_eq!(bar_out_of_range, None);
    assert_eq!(missing_currency, None);
    assert_eq!(
        missing_currency_error,
        crate::cache::CurrencyLookupError::not_found(missing_currency_code)
    );
}

#[rstest]
fn test_data_actor_cache_api_returns_owned_market_data_collection_reads(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    quote_audusd: QuoteTick,
    stub_bar: Bar,
    stub_instrument_status: InstrumentStatus,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache.clone()).unwrap();

    let instrument_id = audusd_sim.id;
    let quote_earlier = QuoteTick {
        instrument_id,
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(2),
        ..quote_audusd
    };
    let quote_latest = QuoteTick {
        instrument_id,
        bid_price: Price::from("1.00010"),
        ask_price: Price::from("1.00020"),
        ts_event: UnixNanos::from(3),
        ts_init: UnixNanos::from(4),
        ..quote_audusd
    };
    let trade_earlier = TradeTick {
        instrument_id,
        price: Price::from("1.00030"),
        ts_event: UnixNanos::from(5),
        ts_init: UnixNanos::from(6),
        ..TradeTick::default()
    };
    let trade_latest = TradeTick {
        instrument_id,
        price: Price::from("1.00040"),
        ts_event: UnixNanos::from(7),
        ts_init: UnixNanos::from(8),
        ..TradeTick::default()
    };
    let bar_type = BarType::from(format!("{instrument_id}-1-MINUTE-BID-EXTERNAL").as_str());
    let bar_earlier = Bar {
        bar_type,
        ts_event: UnixNanos::from(9),
        ts_init: UnixNanos::from(10),
        ..stub_bar
    };
    let bar_latest = Bar {
        bar_type,
        close: Price::from("1.00010"),
        ts_event: UnixNanos::from(11),
        ts_init: UnixNanos::from(12),
        ..stub_bar
    };
    let mark_price_earlier = MarkPriceUpdate::new(
        instrument_id,
        Price::from("1.00050"),
        UnixNanos::from(13),
        UnixNanos::from(14),
    );
    let mark_price_latest = MarkPriceUpdate::new(
        instrument_id,
        Price::from("1.00060"),
        UnixNanos::from(15),
        UnixNanos::from(16),
    );
    let index_price_earlier = IndexPriceUpdate::new(
        instrument_id,
        Price::from("1.00070"),
        UnixNanos::from(17),
        UnixNanos::from(18),
    );
    let index_price_latest = IndexPriceUpdate::new(
        instrument_id,
        Price::from("1.00080"),
        UnixNanos::from(19),
        UnixNanos::from(20),
    );
    let funding_rate_earlier = FundingRateUpdate::new(
        instrument_id,
        dec!(0.0001),
        None,
        None,
        UnixNanos::from(21),
        UnixNanos::from(22),
    );
    let funding_rate_latest = FundingRateUpdate::new(
        instrument_id,
        dec!(0.0002),
        None,
        None,
        UnixNanos::from(23),
        UnixNanos::from(24),
    );
    let status_earlier = InstrumentStatus {
        instrument_id,
        ts_event: UnixNanos::from(25),
        ts_init: UnixNanos::from(26),
        ..stub_instrument_status
    };
    let status_latest = InstrumentStatus {
        instrument_id,
        ts_event: UnixNanos::from(27),
        ts_init: UnixNanos::from(28),
        ..stub_instrument_status
    };
    let synthetic_one_formula = format!("{instrument_id} * 1.0");
    let synthetic_one = SyntheticInstrument::new(
        Symbol::from("SYN"),
        5,
        vec![instrument_id],
        &synthetic_one_formula,
        UnixNanos::from(29),
        UnixNanos::from(30),
    );
    let synthetic_two_formula = format!("{instrument_id} * 2.0");
    let synthetic_two = SyntheticInstrument::new(
        Symbol::from("SYN2"),
        5,
        vec![instrument_id],
        &synthetic_two_formula,
        UnixNanos::from(31),
        UnixNanos::from(32),
    );
    let synthetic_one_id = synthetic_one.id;
    let synthetic_two_id = synthetic_two.id;
    let expected_order_book = {
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
        book.update_quote_tick(&quote_latest).unwrap();
        book
    };

    {
        let mut cache = cache.borrow_mut();
        cache.add_order_book(expected_order_book.clone()).unwrap();
        cache.add_quote(quote_earlier).unwrap();
        cache.add_quote(quote_latest).unwrap();
        cache.add_trade(trade_earlier).unwrap();
        cache.add_trade(trade_latest).unwrap();
        cache.add_bar(bar_earlier).unwrap();
        cache.add_bar(bar_latest).unwrap();
        cache.add_mark_price(mark_price_earlier).unwrap();
        cache.add_mark_price(mark_price_latest).unwrap();
        cache.add_index_price(index_price_earlier).unwrap();
        cache.add_index_price(index_price_latest).unwrap();
        cache.add_funding_rate(funding_rate_earlier).unwrap();
        cache.add_funding_rate(funding_rate_latest).unwrap();
        cache.add_instrument_status(status_earlier).unwrap();
        cache.add_instrument_status(status_latest).unwrap();
        cache.add_synthetic(synthetic_one).unwrap();
        cache.add_synthetic(synthetic_two).unwrap();
    }

    let cache_api = actor.cache();
    let cached_quotes = cache_api.quotes(&instrument_id).unwrap();
    let cached_trades = cache_api.trades(&instrument_id).unwrap();
    let cached_bars = cache_api.bars(&bar_type).unwrap();
    let cached_mark_prices = cache_api.mark_prices(&instrument_id).unwrap();
    let cached_index_prices = cache_api.index_prices(&instrument_id).unwrap();
    let cached_funding_rates = cache_api.funding_rates(&instrument_id).unwrap();
    let cached_statuses = cache_api.instrument_statuses(&instrument_id).unwrap();
    let cached_order_book = cache_api.order_book(&instrument_id).unwrap();
    let cached_synthetics = cache_api.synthetics();
    let missing_instrument_id = InstrumentId::from("MISSING.SIM");
    let missing_bar_type = BarType::from("MISSING.SIM-1-MINUTE-BID-EXTERNAL");
    let missing_quotes = cache_api.quotes(&missing_instrument_id);
    let missing_trades = cache_api.trades(&missing_instrument_id);
    let missing_bars = cache_api.bars(&missing_bar_type);
    let missing_order_book = cache_api.order_book(&missing_instrument_id);

    let _cache_write = cache.borrow_mut();
    let cached_synthetic_ids: AHashSet<InstrumentId> = cached_synthetics
        .iter()
        .map(|synthetic| synthetic.id)
        .collect();
    let expected_synthetic_ids: AHashSet<InstrumentId> =
        [synthetic_one_id, synthetic_two_id].into_iter().collect();
    let cached_synthetic_one = cached_synthetics
        .iter()
        .find(|synthetic| synthetic.id == synthetic_one_id)
        .unwrap();
    let cached_synthetic_two = cached_synthetics
        .iter()
        .find(|synthetic| synthetic.id == synthetic_two_id)
        .unwrap();

    assert_eq!(cached_quotes, vec![quote_latest, quote_earlier]);
    assert_eq!(cached_trades, vec![trade_latest, trade_earlier]);
    assert_eq!(cached_bars, vec![bar_latest, bar_earlier]);
    assert_eq!(
        cached_mark_prices,
        vec![mark_price_latest, mark_price_earlier]
    );
    assert_eq!(
        cached_index_prices,
        vec![index_price_latest, index_price_earlier]
    );
    assert_eq!(
        cached_funding_rates,
        vec![funding_rate_latest, funding_rate_earlier]
    );
    assert_eq!(cached_statuses, vec![status_latest, status_earlier]);
    assert_eq!(cached_order_book, expected_order_book);
    assert_eq!(
        cached_order_book.update_count,
        expected_order_book.update_count
    );
    assert_eq!(cached_synthetic_ids, expected_synthetic_ids);
    assert_eq!(cached_synthetic_one.formula, synthetic_one_formula);
    assert_eq!(cached_synthetic_two.formula, synthetic_two_formula);
    assert_eq!(missing_quotes, None);
    assert_eq!(missing_trades, None);
    assert_eq!(missing_bars, None);
    assert_eq!(missing_order_book, None);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_data_actor_cache_api_returns_owned_pool(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache.clone()).unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x12; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let pool_address = Address::from([0x12; 20]);
    let pool = Pool::new(
        chain,
        Arc::new(dex),
        pool_address,
        PoolIdentifier::from_address(pool_address),
        1000000,
        token0,
        token1,
        Some(3000),
        Some(60),
        UnixNanos::from(1),
    );
    let instrument_id = pool.instrument_id;
    let pool_profiler = PoolProfiler::new(Arc::new(pool.clone()));

    {
        let mut cache = cache.borrow_mut();
        cache.add_pool(pool.clone()).unwrap();
        cache.add_pool_profiler(pool_profiler).unwrap();
    }

    let cache_api = actor.cache();
    let cached_pool = cache_api.pool(&instrument_id);
    let cached_pool_ids = cache_api.pool_ids(Some(&instrument_id.venue));
    let cached_pools = cache_api.pools(Some(&instrument_id.venue));
    let cached_pool_profiler = cache_api.pool_profiler(&instrument_id).unwrap();
    let cached_pool_profiler_ids = cache_api.pool_profiler_ids(Some(&instrument_id.venue));
    let cached_pool_profilers = cache_api.pool_profilers(Some(&instrument_id.venue));

    let _cache_write = cache.borrow_mut();

    assert_eq!(cached_pool, Some(pool));
    assert_eq!(cached_pool_ids, vec![instrument_id]);
    assert_eq!(cached_pools.len(), 1);
    assert_eq!(cached_pools[0].instrument_id, instrument_id);
    assert_eq!(cached_pool_profiler.pool.instrument_id, instrument_id);
    assert_eq!(cached_pool_profiler_ids, vec![instrument_id]);
    assert_eq!(cached_pool_profilers.len(), 1);
    assert_eq!(cached_pool_profilers[0].pool.instrument_id, instrument_id);
}

#[rstest]
fn test_data_actor_cache_api_surface_returns_owned_values(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.register(trader_id, clock, cache).unwrap();

    let instrument_id = audusd_sim.id;
    let venue = instrument_id.venue;
    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL");
    let currency = Currency::USD();
    let currency_code = currency.code;
    let account_id = AccountId::from("ACC-001");
    let client_order_id = ClientOrderId::from("O-001");
    let position_id = PositionId::from("P-001");
    let strategy_id = StrategyId::from("S-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let cache_api = actor.cache();

    let _: Option<InstrumentAny> = cache_api.instrument(&instrument_id);
    let _: Result<InstrumentAny, crate::cache::InstrumentLookupError> =
        cache_api.try_instrument(&instrument_id);
    let _: Vec<InstrumentId> = cache_api.instrument_ids(Some(&venue));
    let _: Vec<InstrumentAny> = cache_api.instruments(&venue, None);
    let _: Vec<InstrumentId> = cache_api.synthetic_ids();
    let _: Option<SyntheticInstrument> = cache_api.synthetic(&instrument_id);
    let _: Vec<SyntheticInstrument> = cache_api.synthetics();
    let _: Option<Price> = cache_api.price(&instrument_id, PriceType::Bid);
    let _: Option<QuoteTick> = cache_api.quote(&instrument_id);
    let _: Option<QuoteTick> = cache_api.quote_at_index(&instrument_id, 0);
    let _: Option<TradeTick> = cache_api.trade(&instrument_id);
    let _: Option<TradeTick> = cache_api.trade_at_index(&instrument_id, 0);
    let _: Option<Bar> = cache_api.bar(&bar_type);
    let _: Option<Bar> = cache_api.bar_at_index(&bar_type, 0);
    let _: Option<MarkPriceUpdate> = cache_api.mark_price(&instrument_id);
    let _: Option<IndexPriceUpdate> = cache_api.index_price(&instrument_id);
    let _: Option<FundingRateUpdate> = cache_api.funding_rate(&instrument_id);
    let _: Option<InstrumentStatus> = cache_api.instrument_status(&instrument_id);
    let _: Option<Vec<QuoteTick>> = cache_api.quotes(&instrument_id);
    let _: Option<Vec<TradeTick>> = cache_api.trades(&instrument_id);
    let _: Option<Vec<Bar>> = cache_api.bars(&bar_type);
    let _: Option<Vec<MarkPriceUpdate>> = cache_api.mark_prices(&instrument_id);
    let _: Option<Vec<IndexPriceUpdate>> = cache_api.index_prices(&instrument_id);
    let _: Option<Vec<FundingRateUpdate>> = cache_api.funding_rates(&instrument_id);
    let _: Option<Vec<InstrumentStatus>> = cache_api.instrument_statuses(&instrument_id);
    let _: Option<OrderBook> = cache_api.order_book(&instrument_id);
    let _: usize = cache_api.book_update_count(&instrument_id);
    let _: usize = cache_api.quote_count(&instrument_id);
    let _: usize = cache_api.trade_count(&instrument_id);
    let _: usize = cache_api.bar_count(&bar_type);
    let _: bool = cache_api.has_order_book(&instrument_id);
    let _: bool = cache_api.has_quote_ticks(&instrument_id);
    let _: bool = cache_api.has_trade_ticks(&instrument_id);
    let _: bool = cache_api.has_bars(&bar_type);
    let _: Option<Currency> = cache_api.currency(&currency_code);
    let _: Result<Currency, crate::cache::CurrencyLookupError> =
        cache_api.try_currency(&currency_code);
    let _: Option<Decimal> = cache_api.get_xrate(venue, currency, currency, PriceType::Mid);
    let _: Option<f64> = cache_api.get_mark_xrate(currency, currency);
    let _: Option<GreeksData> = cache_api.greeks(&instrument_id);
    let _: Option<OptionGreeks> = cache_api.option_greeks(&instrument_id);
    let _: Option<OwnOrderBook> = cache_api.own_order_book(&instrument_id);
    #[cfg(feature = "defi")]
    let _: Option<Pool> = cache_api.pool(&instrument_id);
    #[cfg(feature = "defi")]
    let _: Vec<InstrumentId> = cache_api.pool_ids(Some(&venue));
    #[cfg(feature = "defi")]
    let _: Vec<Pool> = cache_api.pools(Some(&venue));
    #[cfg(feature = "defi")]
    let _: Vec<InstrumentId> = cache_api.pool_profiler_ids(Some(&venue));
    #[cfg(feature = "defi")]
    let _: Option<PoolProfiler> = cache_api.pool_profiler(&instrument_id);
    #[cfg(feature = "defi")]
    let _: Vec<PoolProfiler> = cache_api.pool_profilers(Some(&venue));
    let _: Option<AccountAny> = cache_api.account(&account_id);
    let _: Option<AccountAny> = cache_api.account_for_venue(&venue);
    let _: Option<AccountId> = cache_api.account_id(&venue);
    let _: AHashSet<ComponentId> = cache_api.actor_ids();
    let _: AHashSet<StrategyId> = cache_api.strategy_ids();
    let _: AHashSet<ExecAlgorithmId> = cache_api.exec_algorithm_ids();
    let _: Option<OrderAny> = cache_api.order(&client_order_id);
    let _: Result<OrderAny, crate::cache::OrderLookupError> = cache_api.try_order(&client_order_id);
    let _: bool = cache_api.order_exists(&client_order_id);
    let _: bool = cache_api.is_order_open(&client_order_id);
    let _: bool = cache_api.is_order_closed(&client_order_id);
    let _: bool = cache_api.is_order_active_local(&client_order_id);
    let _: bool = cache_api.is_order_emulated(&client_order_id);
    let _: bool = cache_api.is_order_inflight(&client_order_id);
    let _: Option<ClientOrderId> = cache_api.client_order_id(&venue_order_id);
    let _: Option<VenueOrderId> = cache_api.venue_order_id(&client_order_id);
    let _: Option<ClientId> = cache_api.client_id(&client_order_id);
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids_open(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids_closed(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids_active_local(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids_emulated(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<ClientOrderId> = cache_api.client_order_ids_inflight(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: Vec<OrderAny> = cache_api.orders_open(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: Vec<OrderAny> = cache_api.orders_active_local(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: Vec<OrderAny> = cache_api.orders_emulated(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: Vec<OrderAny> = cache_api.orders_inflight(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: usize = cache_api.orders_open_count(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: usize = cache_api.orders_inflight_count(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: bool = cache_api.has_orders_open(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: bool = cache_api.has_orders_active_local(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: bool = cache_api.has_orders_emulated(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: bool = cache_api.has_orders_inflight(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(OrderSide::Buy),
    );
    let _: Vec<OrderList> =
        cache_api.order_lists(None, Some(&instrument_id), Some(&strategy_id), None);
    let _: Option<Position> = cache_api.position(&position_id);
    let _: Option<Position> = cache_api.position_for_order(&client_order_id);
    let _: Option<PositionId> = cache_api.position_id(&client_order_id);
    let _: bool = cache_api.position_exists(&position_id);
    let _: bool = cache_api.is_position_open(&position_id);
    let _: AHashSet<PositionId> = cache_api.position_ids(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<PositionId> = cache_api.position_open_ids(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: AHashSet<PositionId> = cache_api.position_closed_ids(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
    );
    let _: Vec<Position> = cache_api.positions_open(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(PositionSide::Long),
    );
    let _: usize = cache_api.positions_open_count(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(PositionSide::Long),
    );
    let _: bool = cache_api.has_positions_open(
        None,
        Some(&instrument_id),
        Some(&strategy_id),
        Some(&account_id),
        Some(PositionSide::Long),
    );
}

/// Helper to register a dummy actor and return its Rc.
fn register_dummy(name: &str) -> Rc<UnsafeCell<dyn Actor>> {
    let actor = DummyActor::new(name);
    register_actor(actor)
}

#[rstest]
#[case("actor-001")]
#[case("actor-002")]
fn test_register_and_get(#[case] name: &str) {
    let rc = register_dummy(name);
    // Retrieve by id
    let id = unsafe { &*rc.get() }.id();
    let found = get_actor(&id).expect("actor not found");
    // Should be same Rc pointer
    assert!(Rc::ptr_eq(&rc, &found));
}

#[rstest]
fn test_get_nonexistent() {
    let id = Ustr::from_str("no_such_actor").unwrap();
    assert!(get_actor(&id).is_none());
}

#[should_panic(expected = "Actor for")]
#[rstest]
fn test_get_actor_unchecked_panic() {
    let id = Ustr::from_str("unknown").unwrap();
    // Should panic due to missing actor
    let _guard = get_actor_unchecked::<DummyActor>(&id);
}

#[rstest]
fn test_get_actor_unchecked_mutate() {
    let name = "mutant";
    let _rc = register_dummy(name);
    let id = Ustr::from_str(name).unwrap();

    // Mutate via unchecked - must scope the borrow
    {
        let mut actor_ref = get_actor_unchecked::<DummyActor>(&id);
        actor_ref.count = 42;
    } // Guard dropped here, releasing borrow

    // Read back via unchecked again (now allowed since previous borrow dropped)
    let actor_ref2 = get_actor_unchecked::<DummyActor>(&id);
    assert_eq!(actor_ref2.count, 42);
}

#[rstest]
fn test_subscribe_and_receive_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(TestActorCustomData::type_name_static(), None, None);
    actor.subscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let data = make_test_custom_data("CustomData-01");
    msgbus::publish_any(topic, &data);
    let data = make_test_custom_data("CustomData-02");
    msgbus::publish_any(topic, &data);

    assert_eq!(actor.received_data.len(), 2);
}

#[rstest]
fn test_unsubscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(TestActorCustomData::type_name_static(), None, None);
    actor.subscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let data = make_test_custom_data("CustomData-01");
    msgbus::publish_any(topic, &data);
    let data = make_test_custom_data("CustomData-02");
    msgbus::publish_any(topic, &data);

    actor.unsubscribe_data(data_type, None, None);

    // Publish more data
    let data = make_test_custom_data("CustomData-03");
    msgbus::publish_any(topic, &data);
    let data = make_test_custom_data("CustomData-04");
    msgbus::publish_any(topic, &data);

    // Actor should not receive new data
    assert_eq!(actor.received_data.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_book_deltas(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);

    let topic = get_book_deltas_topic(audusd_sim.id);

    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.00000"),
        Quantity::from("100000"),
        123456,
    );
    let delta = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

    msgbus::publish_deltas(topic, &deltas);

    assert_eq!(actor.received_deltas.len(), 1);
}

fn parent_params() -> Params {
    let mut params = Params::new();
    params.insert(PARAMS_IS_PARENT.to_string(), serde_json::json!(true));
    params
}

#[rstest]
fn test_parent_book_deltas_subscription_receives_per_underlying(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let parent_id = InstrumentId::from("ES.FUT.XCME");
    let underlying_id = InstrumentId::from("ESZ24.XCME");

    actor.subscribe_book_deltas(
        parent_id,
        BookType::L2_MBP,
        None,
        None,
        false,
        Some(parent_params()),
    );

    let underlying_topic = get_book_deltas_topic(underlying_id);

    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("4000.00"),
        Quantity::from("1"),
        123456,
    );
    let delta = OrderBookDelta::new(
        underlying_id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(underlying_id, vec![delta]);

    msgbus::publish_deltas(underlying_topic, &deltas);

    assert_eq!(actor.received_deltas.len(), 1);
}

#[rstest]
fn test_parent_book_deltas_unsubscribe_removes_per_underlying_handler(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let parent_id = InstrumentId::from("ES.FUT.XCME");
    let underlying_id = InstrumentId::from("ESZ24.XCME");

    actor.subscribe_book_deltas(
        parent_id,
        BookType::L2_MBP,
        None,
        None,
        false,
        Some(parent_params()),
    );
    assert_eq!(actor.deltas_handler_count(), 1);

    actor.unsubscribe_book_deltas(parent_id, None, Some(parent_params()));
    assert_eq!(actor.deltas_handler_count(), 0);

    let underlying_topic = get_book_deltas_topic(underlying_id);
    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("4000.00"),
        Quantity::from("1"),
        123456,
    );
    let delta = OrderBookDelta::new(
        underlying_id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(underlying_id, vec![delta]);

    msgbus::publish_deltas(underlying_topic, &deltas);

    assert_eq!(actor.received_deltas.len(), 0);
}

#[rstest]
fn test_betfair_runner_subscription_does_not_cross_leak(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let runner_a = InstrumentId::from("1.211334112-31570229.BETFAIR");
    let runner_b = InstrumentId::from("1.211334112-99887766.BETFAIR");

    actor.subscribe_book_deltas(runner_a, BookType::L2_MBP, None, None, false, None);

    let runner_b_topic = get_book_deltas_topic(runner_b);
    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("2.00"),
        Quantity::from("100"),
        1,
    );
    let delta = OrderBookDelta::new(
        runner_b,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(runner_b, vec![delta]);

    msgbus::publish_deltas(runner_b_topic, &deltas);

    assert_eq!(
        actor.received_deltas.len(),
        0,
        "subscriber for runner A must not receive deltas published for runner B \
         even though their symbols share the leading digit `1`",
    );

    let runner_a_topic = get_book_deltas_topic(runner_a);
    let runner_a_delta = OrderBookDelta::new(
        runner_a,
        BookAction::Add,
        BookOrder::new(OrderSide::Buy, Price::from("3.00"), Quantity::from("50"), 2),
        0,
        1,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    let runner_a_deltas = OrderBookDeltas::new(runner_a, vec![runner_a_delta]);

    msgbus::publish_deltas(runner_a_topic, &runner_a_deltas);

    assert_eq!(
        actor.received_deltas.len(),
        1,
        "subscriber for runner A must receive deltas published on runner A",
    );
    assert_eq!(actor.received_deltas[0].instrument_id, runner_a);
}

#[rstest]
fn test_unsubscribe_book_deltas(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);

    let topic = get_book_deltas_topic(audusd_sim.id);

    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.00000"),
        Quantity::from("100000"),
        123456,
    );
    let delta = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

    msgbus::publish_deltas(topic, &deltas);

    // Unsubscribe
    actor.unsubscribe_book_deltas(audusd_sim.id, None, None);

    let delta2 = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        2,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    let deltas2 = OrderBookDeltas::new(audusd_sim.id, vec![delta2]);

    // Publish again
    msgbus::publish_deltas(topic, &deltas2);

    // Should still only have one delta
    assert_eq!(actor.received_deltas.len(), 1);
}

#[rstest]
fn test_subscribe_and_receive_book_at_interval(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, interval_ms, None, None);

    let topic = get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish_book(topic, &book);

    assert_eq!(actor.received_books.len(), 1);
}

#[rstest]
fn test_unsubscribe_book_at_interval(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, interval_ms, None, None);

    let topic = get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish_book(topic, &book);

    assert_eq!(actor.received_books.len(), 1);

    actor.unsubscribe_book_at_interval(audusd_sim.id, interval_ms, None, None);

    // Publish more book refs
    msgbus::publish_book(topic, &book);
    msgbus::publish_book(topic, &book);

    // Should still only have one book
    assert_eq!(actor.received_books.len(), 1);
}

#[rstest]
fn test_unsubscribe_book_at_interval_keeps_other_intervals(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let fast_interval_ms = NonZeroUsize::new(500).unwrap();
    let slow_interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, fast_interval_ms, None, None);
    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, slow_interval_ms, None, None);

    let fast_topic = get_book_snapshots_topic(audusd_sim.id, fast_interval_ms);
    let slow_topic = get_book_snapshots_topic(audusd_sim.id, slow_interval_ms);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish_book(fast_topic, &book);
    msgbus::publish_book(slow_topic, &book);

    assert_eq!(actor.received_books.len(), 2);

    actor.unsubscribe_book_at_interval(audusd_sim.id, fast_interval_ms, None, None);

    msgbus::publish_book(fast_topic, &book);
    msgbus::publish_book(slow_topic, &book);

    assert_eq!(actor.received_books.len(), 3);
}

#[rstest]
fn test_subscribe_and_receive_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish_quote(topic, &quote);
    msgbus::publish_quote(topic, &quote);

    assert_eq!(actor.received_quotes.len(), 2);
}

#[rstest]
fn test_unsubscribe_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish_quote(topic, &quote);
    msgbus::publish_quote(topic, &quote);

    actor.unsubscribe_quotes(audusd_sim.id, None, None);

    // Publish more quotes
    msgbus::publish_quote(topic, &quote);
    msgbus::publish_quote(topic, &quote);

    // Actor should not receive new quotes
    assert_eq!(actor.received_quotes.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish_trade(topic, &trade);
    msgbus::publish_trade(topic, &trade);

    assert_eq!(actor.received_trades.len(), 2);
}

#[rstest]
fn test_unsubscribe_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish_trade(topic, &trade);
    msgbus::publish_trade(topic, &trade);

    actor.unsubscribe_trades(audusd_sim.id, None, None);

    // Publish more trades
    msgbus::publish_trade(topic, &trade);
    msgbus::publish_trade(topic, &trade);

    // Actor should not receive new trades
    assert_eq!(actor.received_trades.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);

    let topic = get_bars_topic(bar_type);
    let bar = Bar::default();
    msgbus::publish_bar(topic, &bar);

    assert_eq!(actor.received_bars.len(), 1);
}

#[rstest]
fn test_unsubscribe_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);

    let topic = get_bars_topic(bar_type);
    let bar = Bar::default();
    msgbus::publish_bar(topic, &bar);

    // Unsubscribe
    actor.unsubscribe_bars(bar_type, None, None);

    // Publish more bars
    msgbus::publish_bar(topic, &bar);
    msgbus::publish_bar(topic, &bar);

    // Should still only have one bar
    assert_eq!(actor.received_bars.len(), 1);
}

#[rstest]
fn test_request_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_instrument(audusd_sim.id, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument = InstrumentAny::CurrencyPair(audusd_sim.clone());
    let data = instrument.clone();
    let ts_init = UnixNanos::default();
    let response = InstrumentResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    let data_response = DataResponse::Instrument(Box::new(response));
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_instruments.len(), 1);
    assert_eq!(actor.received_instruments[0], instrument);
}

#[rstest]
fn test_request_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::test_default();
    let request_id = actor
        .request_instruments(Some(venue), None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument1 = InstrumentAny::CurrencyPair(audusd_sim);
    let instrument2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    let data = vec![instrument1.clone(), instrument2.clone()];
    let ts_init = UnixNanos::default();
    let response = InstrumentsResponse::new(
        request_id,
        client_id,
        venue,
        data,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    let data_response = DataResponse::Instruments(response);
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], instrument1);
    assert_eq!(actor.received_instruments[1], instrument2);
}

#[rstest]
fn test_request_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_quotes(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let quote = QuoteTick::default();
    let data = vec![quote];
    let ts_init = UnixNanos::default();
    let response = QuotesResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(1_690_000_000_000_000_000)),
        Some(UnixNanos::from(1_700_000_000_000_000_000)),
        ts_init,
        None,
    );

    let data_response = DataResponse::Quotes(response);
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_quotes.len(), 1);
    assert_eq!(actor.received_quotes[0], quote);
}

#[rstest]
fn test_request_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_trades(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let trade = TradeTick::default();
    let data = vec![trade];
    let ts_init = UnixNanos::default();
    let response = TradesResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(1_695_000_000_000_000_000)),
        Some(UnixNanos::from(1_699_000_000_000_000_000)),
        ts_init,
        None,
    );

    let data_response = DataResponse::Trades(response);
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_trades.len(), 1);
    assert_eq!(actor.received_trades[0], trade);
}

#[rstest]
fn test_request_book_deltas(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_book_deltas(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let delta = stub_delta();
    let response = BookDeltasResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        vec![delta],
        None,
        None,
        UnixNanos::default(),
        None,
    );

    msgbus::send_response(&request_id, &DataResponse::BookDeltas(response));

    assert_eq!(actor.received_deltas.len(), 1);
    assert_eq!(actor.received_deltas[0], delta);
}

#[rstest]
fn test_request_book_depth(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_book_depth(audusd_sim.id, None, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let mut depth = stub_depth10();
    depth.instrument_id = audusd_sim.id;
    let response = BookDepthResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        vec![depth],
        None,
        None,
        UnixNanos::default(),
        None,
    );

    msgbus::send_response(&request_id, &DataResponse::BookDepth(response));

    assert_eq!(actor.received_depths.len(), 1);
    assert_eq!(actor.received_depths[0], depth);
}

#[rstest]
fn test_request_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_funding_rates(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        dec!(0.0001),
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let data = vec![funding_rate];
    let ts_init = UnixNanos::default();
    let response = FundingRatesResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(1_695_000_000_000_000_000)),
        Some(UnixNanos::from(1_699_000_000_000_000_000)),
        ts_init,
        None,
    );

    let data_response = DataResponse::FundingRates(response);
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_funding_rates.len(), 1);
    assert_eq!(actor.received_funding_rates[0], funding_rate);
}

#[rstest]
fn test_request_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    let request_id = actor
        .request_bars(bar_type, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let bar_type = BarType::from_str("AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL").unwrap();
    let bar = Bar::default();
    let data = vec![bar];
    let ts_init = UnixNanos::default();
    let response = BarsResponse::new(
        request_id,
        client_id,
        bar_type,
        data,
        Some(UnixNanos::from(1_700_000_000_000_000_000)),
        Some(UnixNanos::from(1_705_000_000_000_000_000)),
        ts_init,
        None,
    );

    let data_response = DataResponse::Bars(response);
    msgbus::send_response(&request_id, &data_response);

    assert_eq!(actor.received_bars.len(), 1);
    assert_eq!(actor.received_bars[0], bar);
}

#[rstest]
fn test_subscribe_and_receive_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::test_default();
    actor.subscribe_instruments(venue, None, None);

    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    let topic1 = get_instrument_topic(inst1.id());
    msgbus::publish_instrument(topic1, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    let topic2 = get_instrument_topic(inst2.id());
    msgbus::publish_instrument(topic2, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], inst1);
    assert_eq!(actor.received_instruments[1], inst2);
}

#[rstest]
fn test_subscribe_and_receive_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_instrument(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish_instrument(topic, &inst1);
    msgbus::publish_instrument(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], inst1);
    assert_eq!(actor.received_instruments[1], inst2);
}

#[rstest]
fn test_subscribe_and_receive_mark_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices(audusd_sim.id, None, None);

    let topic = get_mark_price_topic(audusd_sim.id);
    let mp1 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_mark_price(topic, &mp1);
    let mp2 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish_mark_price(topic, &mp2);

    assert_eq!(actor.received_mark_prices.len(), 2);
    assert_eq!(actor.received_mark_prices[0], mp1);
    assert_eq!(actor.received_mark_prices[1], mp2);
}

#[rstest]
fn test_subscribe_and_receive_index_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices(audusd_sim.id, None, None);

    let topic = get_index_price_topic(audusd_sim.id);
    let ip = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_index_price(topic, &ip);

    assert_eq!(actor.received_index_prices.len(), 1);
    assert_eq!(actor.received_index_prices[0], ip);
}

#[rstest]
fn test_subscribe_and_receive_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_funding_rates(audusd_sim.id, None, None);

    let topic = get_funding_rate_topic(audusd_sim.id);
    let fr1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_funding_rate(topic, &fr1);
    let fr2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish_funding_rate(topic, &fr2);

    assert_eq!(actor.received_funding_rates.len(), 2);
    assert_eq!(actor.received_funding_rates[0], fr1);
    assert_eq!(actor.received_funding_rates[1], fr2);
}

#[rstest]
fn test_subscribe_and_receive_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status(instrument_id, None, None);

    let topic = get_instrument_status_topic(instrument_id);
    msgbus::publish_any(topic, &stub_instrument_status);

    assert_eq!(actor.received_status.len(), 1);
    assert_eq!(actor.received_status[0], stub_instrument_status);
}

#[rstest]
fn test_subscribe_and_receive_instrument_close(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_close: InstrumentClose,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close(instrument_id, None, None);

    let topic = get_instrument_close_topic(instrument_id);
    msgbus::publish_any(topic, &stub_instrument_close);

    assert_eq!(actor.received_closes.len(), 1);
    assert_eq!(actor.received_closes[0], stub_instrument_close);
}

#[rstest]
fn test_subscribe_and_receive_option_greeks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = InstrumentId::from("AAPL-20250321-200C.OPRA");
    actor.subscribe_option_greeks(instrument_id, None, None);

    let greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.55,
            gamma: 0.03,
            vega: 0.12,
            theta: -0.05,
            rho: 0.01,
        },
        mark_iv: Some(0.25),
        bid_iv: Some(0.24),
        ask_iv: Some(0.26),
        underlying_price: Some(195.0),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    };

    let topic = get_option_greeks_topic(instrument_id);
    msgbus::publish_option_greeks(topic, &greeks);

    assert_eq!(actor.received_greeks.len(), 1);
    assert_eq!(actor.received_greeks[0], greeks);
}

#[rstest]
fn test_subscribe_and_receive_option_chain(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let series_id = OptionSeriesId::new(
        Venue::from("OPRA"),
        Ustr::from("AAPL"),
        Ustr::from("USD"),
        UnixNanos::from(1_711_036_800_000_000_000),
    );
    let strike_range = StrikeRange::AtmRelative {
        strikes_above: 5,
        strikes_below: 5,
    };
    actor.subscribe_option_chain(series_id, strike_range, None, None, None);

    let slice = OptionChainSlice {
        series_id,
        atm_strike: Some(Price::from("200.00")),
        calls: Default::default(),
        puts: Default::default(),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    };

    let topic = get_option_chain_topic(series_id);
    msgbus::publish_option_chain(topic, &slice);

    assert_eq!(actor.received_chain_slices.len(), 1);
    assert_eq!(actor.received_chain_slices[0].series_id, series_id);
}

#[rstest]
fn test_unsubscribe_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::test_default();
    actor.subscribe_instruments(venue, None, None);

    let inst1 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    let topic1 = get_instrument_topic(inst1.id());
    msgbus::publish_instrument(topic1, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
    let topic2 = get_instrument_topic(inst2.id());
    msgbus::publish_instrument(topic2, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instruments(venue, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish_instrument(topic1, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish_instrument(topic2, &inst4);

    assert_eq!(actor.received_instruments.len(), 2);
}

#[rstest]
fn test_unsubscribe_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();
    actor.subscribe_instrument(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst3 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish_instrument(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
    msgbus::publish_instrument(topic, &inst4);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instrument(audusd_sim.id, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish_instrument(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish_instrument(topic, &inst4);

    assert_eq!(actor.received_instruments.len(), 2);
}

#[rstest]
fn test_unsubscribe_mark_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices(audusd_sim.id, None, None);

    let topic = get_mark_price_topic(audusd_sim.id);
    let mp1 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_mark_price(topic, &mp1);
    let mp2 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish_mark_price(topic, &mp2);

    assert_eq!(actor.received_mark_prices.len(), 2);

    actor.unsubscribe_mark_prices(audusd_sim.id, None, None);

    let mp3 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00020"),
        UnixNanos::from(5),
        UnixNanos::from(6),
    );
    msgbus::publish_mark_price(topic, &mp3);
    let mp4 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00030"),
        UnixNanos::from(7),
        UnixNanos::from(8),
    );
    msgbus::publish_mark_price(topic, &mp4);

    assert_eq!(actor.received_mark_prices.len(), 2);
}

#[rstest]
fn test_unsubscribe_index_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices(audusd_sim.id, None, None);

    let topic = get_index_price_topic(audusd_sim.id);
    let ip1 = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_index_price(topic, &ip1);

    assert_eq!(actor.received_index_prices.len(), 1);

    actor.unsubscribe_index_prices(audusd_sim.id, None, None);

    let ip2 = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish_index_price(topic, &ip2);

    assert_eq!(actor.received_index_prices.len(), 1);
}

#[rstest]
fn test_unsubscribe_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_funding_rates(audusd_sim.id, None, None);

    let topic = get_funding_rate_topic(audusd_sim.id);
    let fr1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish_funding_rate(topic, &fr1);

    assert_eq!(actor.received_funding_rates.len(), 1);

    actor.unsubscribe_funding_rates(audusd_sim.id, None, None);

    let fr2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish_funding_rate(topic, &fr2);

    assert_eq!(actor.received_funding_rates.len(), 1);
}

#[rstest]
fn test_unsubscribe_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status(instrument_id, None, None);

    let topic = get_instrument_status_topic(instrument_id);
    msgbus::publish_any(topic, &stub_instrument_status);

    assert_eq!(actor.received_status.len(), 1);

    actor.unsubscribe_instrument_status(instrument_id, None, None);

    let stub2 = stub_instrument_status;
    msgbus::publish_any(topic, &stub2);

    assert_eq!(actor.received_status.len(), 1);
}

#[rstest]
fn test_unsubscribe_instrument_close(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_close: InstrumentClose,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close(instrument_id, None, None);

    let topic = get_instrument_close_topic(instrument_id);
    msgbus::publish_any(topic, &stub_instrument_close);

    assert_eq!(actor.received_closes.len(), 1);

    actor.unsubscribe_instrument_close(instrument_id, None, None);

    let stub2 = stub_instrument_close;
    msgbus::publish_any(topic, &stub2);

    assert_eq!(actor.received_closes.len(), 1);
}

#[rstest]
fn test_unsubscribe_option_greeks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = InstrumentId::from("AAPL-20250321-200C.OPRA");
    actor.subscribe_option_greeks(instrument_id, None, None);

    let greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.55,
            gamma: 0.03,
            vega: 0.12,
            theta: -0.05,
            rho: 0.01,
        },
        mark_iv: Some(0.25),
        bid_iv: None,
        ask_iv: None,
        underlying_price: None,
        open_interest: None,
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    };

    let topic = get_option_greeks_topic(instrument_id);
    msgbus::publish_option_greeks(topic, &greeks);

    assert_eq!(actor.received_greeks.len(), 1);

    actor.unsubscribe_option_greeks(instrument_id, None, None);

    msgbus::publish_option_greeks(topic, &greeks);

    assert_eq!(actor.received_greeks.len(), 1);
}

#[rstest]
fn test_unsubscribe_option_chain(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let series_id = OptionSeriesId::new(
        Venue::from("OPRA"),
        Ustr::from("AAPL"),
        Ustr::from("USD"),
        UnixNanos::from(1_711_036_800_000_000_000),
    );
    let strike_range = StrikeRange::AtmRelative {
        strikes_above: 5,
        strikes_below: 5,
    };
    actor.subscribe_option_chain(series_id, strike_range, None, None, None);

    let slice = OptionChainSlice {
        series_id,
        atm_strike: None,
        calls: Default::default(),
        puts: Default::default(),
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    };

    let topic = get_option_chain_topic(series_id);
    msgbus::publish_option_chain(topic, &slice);

    assert_eq!(actor.received_chain_slices.len(), 1);

    actor.unsubscribe_option_chain(series_id, None);

    msgbus::publish_option_chain(topic, &slice);

    assert_eq!(actor.received_chain_slices.len(), 1);
}

#[rstest]
fn test_request_book_snapshot(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request a book snapshot
    let request_id = actor
        .request_book_snapshot(audusd_sim.id, None, None, None)
        .unwrap();

    // Build a dummy book and response
    let client_id = ClientId::new("Client2");
    let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);

    // Provide ts_init and no params
    let ts_init = UnixNanos::default();
    let response = BookResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        book.clone(),
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );
    let data_response = DataResponse::Book(response);
    msgbus::send_response(&request_id, &data_response);

    // Should trigger on_book and record the book
    assert_eq!(actor.received_books.len(), 1);
    assert_eq!(actor.received_books[0], book);
}

#[rstest]
fn test_request_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    test_logging();

    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request custom data
    let data_type = DataType::new("TestData", None, None);
    let client_id = ClientId::new("TestClient");
    let request_id = actor
        .request_data(data_type.clone(), client_id, None, None, None, None)
        .unwrap();

    // Build a response payload containing a String
    let payload = Arc::new(Bytes::from("Data-001"));
    let ts_init = UnixNanos::default();

    // Create response with payload type String
    let response = CustomDataResponse::new(
        request_id,
        client_id,
        None,
        data_type,
        payload,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    // Publish the response
    let data_response = DataResponse::Data(response);
    msgbus::send_response(&request_id, &data_response);

    // Actor should receive the custom data
    assert_eq!(actor.received_data.len(), 1);
    assert_eq!(actor.received_data[0], "Any { .. }");
}

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_blocks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let blockchain = Blockchain::Ethereum;
    actor.subscribe_blocks(blockchain, None, None);

    let topic = get_defi_blocks_topic(blockchain);
    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    msgbus::publish_defi_block(topic, &block);

    assert_eq!(actor.received_blocks.len(), 1);
    assert_eq!(actor.received_blocks[0], block);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_unsubscribe_blocks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let blockchain = Blockchain::Ethereum;
    actor.subscribe_blocks(blockchain, None, None);

    let topic = get_defi_blocks_topic(blockchain);
    let block1 = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    msgbus::publish_defi_block(topic, &block1);

    // Unsubscribe
    actor.unsubscribe_blocks(blockchain, None, None);

    let block2 = Block::new(
        "0x789".to_string(),
        "0xabc".to_string(),
        2u64,
        "miner2".into(),
        1000001u64,
        500001u64,
        UnixNanos::from(2),
        Some(blockchain),
    );
    msgbus::publish_defi_block(topic, &block2);

    // Should still only have one block
    assert_eq!(actor.received_blocks.len(), 1);
    assert_eq!(actor.received_blocks[0], block1);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_pools(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x12; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let pool_address = Address::from([0x12; 20]);
    let pool = Pool::new(
        chain,
        Arc::new(dex),
        pool_address,
        PoolIdentifier::from_address(pool_address),
        1000000,
        token0,
        token1,
        Some(3000),
        Some(60),
        UnixNanos::from(1),
    );

    let instrument_id = pool.instrument_id;
    actor.subscribe_pool(instrument_id, None, None);

    let topic = get_defi_pool_topic(instrument_id);

    msgbus::publish_defi_pool(topic, &pool);

    assert_eq!(actor.received_pools.len(), 1);
    assert_eq!(actor.received_pools[0], pool);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_pool_swaps(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );

    let pool_address = Address::from_str("0xC31E54c7A869B9fCbECC14363CF510d1C41Fa443").unwrap();
    let instrument_id =
        InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3");

    let swap = PoolSwap::new(
        chain,
        Arc::new(dex),
        instrument_id,
        PoolIdentifier::from_address(pool_address),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );

    actor.subscribe_pool_swaps(instrument_id, None, None);

    let topic = get_defi_pool_swaps_topic(instrument_id);

    msgbus::publish_defi_swap(topic, &swap);

    assert_eq!(actor.received_pool_swaps.len(), 1);
    assert_eq!(actor.received_pool_swaps[0], swap);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_unsubscribe_pool_swaps(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    let pool_address = Address::from_str("0xC31E54c7A869B9fCbECC14363CF510d1C41Fa443").unwrap();
    let pool_identifier = pool_address.to_string();
    let instrument_id = Pool::create_instrument_id(chain.name, &dex, &pool_identifier);

    actor.subscribe_pool_swaps(instrument_id, None, None);

    let topic = get_defi_pool_swaps_topic(instrument_id);

    let swap1 = PoolSwap::new(
        chain.clone(),
        Arc::new(dex.clone()),
        instrument_id,
        PoolIdentifier::from_address(pool_address),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );
    msgbus::publish_defi_swap(topic, &swap1);

    // Unsubscribe
    actor.unsubscribe_pool_swaps(instrument_id, None, None);

    let swap2 = PoolSwap::new(
        chain,
        Arc::new(dex),
        instrument_id,
        PoolIdentifier::from_address(pool_address),
        2000u64,
        "0x456".to_string(),
        0,
        0,
        UnixNanos::default(),
        UnixNanos::default(),
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );
    msgbus::publish_defi_swap(topic, &swap2);

    // Should still only have one swap
    assert_eq!(actor.received_pool_swaps.len(), 1);
    assert_eq!(actor.received_pool_swaps[0], swap1);
}

#[rstest]
fn test_duplicate_subscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    // Register actor
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Subscribe twice to the same DataType
    let data_type = DataType::new(TestActorCustomData::type_name_static(), None, None);
    actor.subscribe_data(data_type.clone(), None, None);
    actor.subscribe_data(data_type.clone(), None, None);

    // Publish a single message
    let topic = get_custom_topic(&data_type);
    let payload = make_test_custom_data("Custom-XYZ");
    msgbus::publish_any(topic, &payload);

    // Only a single handler should be active despite duplicate subscribe attempt
    assert_eq!(actor.received_data.len(), 1);
}

#[rstest]
fn test_unsubscribe_before_subscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(TestActorCustomData::type_name_static(), None, None);

    // Unsubscribe without prior subscription: should not panic and no data received
    actor.unsubscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let payload = make_test_custom_data("Custom-ABC");
    msgbus::publish_any(topic, &payload);

    assert!(actor.received_data.is_empty());
}

#[derive(Debug)]
struct SaveLoadActor {
    core: DataActorCore,
    loaded_state: Option<IndexMap<String, Vec<u8>>>,
}

impl SaveLoadActor {
    fn new(config: DataActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config),
            loaded_state: None,
        }
    }
}

nautilus_actor!(SaveLoadActor);

impl DataActor for SaveLoadActor {
    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        let mut map = IndexMap::new();
        map.insert("answer".to_string(), vec![4, 2]);
        Ok(map)
    }

    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        self.loaded_state = Some(state);
        Ok(())
    }
}

#[rstest]
#[case::with_reason(Some("graceful exit".to_string()))]
#[case::no_reason(None)]
fn test_shutdown_system_publishes_command(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    #[case] reason: Option<String>,
) {
    use crate::{messages::system::ShutdownSystem, msgbus::typed_handler::ShareableMessageHandler};

    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);

    let received: Rc<RefCell<Vec<ShutdownSystem>>> = Rc::new(RefCell::new(Vec::new()));
    let received_clone = received.clone();
    let handler = ShareableMessageHandler::from_typed(move |cmd: &ShutdownSystem| {
        received_clone.borrow_mut().push(cmd.clone());
    });
    let topic = MessagingSwitchboard::shutdown_system_topic();
    msgbus::subscribe_any(topic.into(), handler, None);

    actor.shutdown_system(reason.clone());

    let received = received.borrow();
    assert_eq!(received.len(), 1);
    let cmd = &received[0];
    assert_eq!(cmd.trader_id, trader_id);
    assert_eq!(cmd.component_id, actor_id);
    assert_eq!(cmd.reason, reason);
}

#[rstest]
fn test_on_save_and_on_load(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let config = DataActorConfig::default();

    // Prepare actor & register
    let mut actor = SaveLoadActor::new(config);
    actor.register(trader_id, clock, cache).unwrap();
    let actor_id = actor.actor_id();
    register_actor(actor);

    // Fetch back to mutate
    let actor_key = actor_id.inner();
    let mut actor_ref = get_actor_unchecked::<SaveLoadActor>(&actor_key);

    // Invoke on_save – emulate persistence snapshot
    let snapshot = actor_ref.on_save().unwrap();
    assert!(snapshot.contains_key("answer"));

    // Invoke on_load with snapshot
    actor_ref.on_load(snapshot.clone()).unwrap();

    // Verify state stored
    assert_eq!(actor_ref.loaded_state.as_ref(), Some(&snapshot));
}

#[rstest]
fn test_data_actor_core_tracks_quote_handlers(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    assert_eq!(actor.quote_handler_count(), 0);

    actor.subscribe_quotes(audusd_sim.id, None, None);

    assert_eq!(actor.quote_handler_count(), 1);

    let topic = get_quotes_topic(audusd_sim.id);
    assert!(actor.has_quote_handler(topic.as_str()));
}

#[rstest]
fn test_data_actor_core_removes_quote_handler_on_unsubscribe(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);
    assert_eq!(actor.quote_handler_count(), 1);

    actor.unsubscribe_quotes(audusd_sim.id, None, None);
    assert_eq!(actor.quote_handler_count(), 0);

    let topic = get_quotes_topic(audusd_sim.id);
    assert!(!actor.has_quote_handler(topic.as_str()));
}

#[rstest]
fn test_data_actor_core_tracks_trade_handlers(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    assert_eq!(actor.trade_handler_count(), 0);

    actor.subscribe_trades(audusd_sim.id, None, None);

    assert_eq!(actor.trade_handler_count(), 1);

    let topic = get_trades_topic(audusd_sim.id);
    assert!(actor.has_trade_handler(topic.as_str()));
}

#[rstest]
fn test_data_actor_core_removes_trade_handler_on_unsubscribe(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades(audusd_sim.id, None, None);
    assert_eq!(actor.trade_handler_count(), 1);

    actor.unsubscribe_trades(audusd_sim.id, None, None);
    assert_eq!(actor.trade_handler_count(), 0);
}

#[rstest]
fn test_data_actor_core_tracks_bar_handlers(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    assert_eq!(actor.bar_handler_count(), 0);

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);

    assert_eq!(actor.bar_handler_count(), 1);

    let topic = get_bars_topic(bar_type);
    assert!(actor.has_bar_handler(topic.as_str()));
}

#[rstest]
fn test_data_actor_core_removes_bar_handler_on_unsubscribe(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);
    assert_eq!(actor.bar_handler_count(), 1);

    actor.unsubscribe_bars(bar_type, None, None);
    assert_eq!(actor.bar_handler_count(), 0);
}

#[rstest]
fn test_data_actor_core_tracks_deltas_handlers(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    assert_eq!(actor.deltas_handler_count(), 0);

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);

    assert_eq!(actor.deltas_handler_count(), 1);

    let topic = get_book_deltas_topic(audusd_sim.id);
    assert!(actor.has_deltas_handler(topic.as_str()));
}

#[rstest]
fn test_data_actor_core_removes_deltas_handler_on_unsubscribe(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);
    assert_eq!(actor.deltas_handler_count(), 1);

    actor.unsubscribe_book_deltas(audusd_sim.id, None, None);
    assert_eq!(actor.deltas_handler_count(), 0);
}

#[rstest]
fn test_data_actor_core_multiple_subscriptions_tracked(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);
    actor.subscribe_quotes(gbpusd_sim.id, None, None);
    actor.subscribe_trades(audusd_sim.id, None, None);

    assert_eq!(actor.quote_handler_count(), 2);
    assert_eq!(actor.trade_handler_count(), 1);

    actor.unsubscribe_quotes(audusd_sim.id, None, None);

    assert_eq!(actor.quote_handler_count(), 1);
    assert_eq!(actor.trade_handler_count(), 1);

    let aud_topic = get_quotes_topic(audusd_sim.id);
    let gbp_topic = get_quotes_topic(gbpusd_sim.id);
    assert!(!actor.has_quote_handler(aud_topic.as_str()));
    assert!(actor.has_quote_handler(gbp_topic.as_str()));
}

#[rstest]
fn test_publish_data_reaches_subscriber(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data = make_test_custom_data("published-42");
    actor.subscribe_data(data.data_type.clone(), None, None);

    actor.publish_data(&data.data_type, &data);

    assert_eq!(actor.received_custom_data.len(), 1);
    assert_eq!(actor.received_custom_data[0], data);
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_publish_data_panics_when_unregistered() {
    let actor = TestDataActor::new(DataActorConfig::default());
    let data = make_test_custom_data("x");
    actor.publish_data(&data.data_type, &data);
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_publish_signal_panics_when_unregistered() {
    let actor = TestDataActor::new(DataActorConfig::default());
    actor.publish_signal("example", "1".to_string(), UnixNanos::default());
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_subscribe_signal_panics_when_unregistered() {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.subscribe_signal("example", None);
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_unsubscribe_signal_panics_when_unregistered() {
    let mut actor = TestDataActor::new(DataActorConfig::default());
    actor.unsubscribe_signal("example");
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_add_synthetic_panics_when_unregistered() {
    use std::str::FromStr;

    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::SyntheticInstrument,
    };

    let actor = TestDataActor::new(DataActorConfig::default());
    let comp1 = InstrumentId::from_str("BTC-USD.VENUE").unwrap();
    let comp2 = InstrumentId::from_str("ETH-USD.VENUE").unwrap();
    let formula = format!("({comp1} + {comp2}) / 2.0");
    let synthetic = SyntheticInstrument::new(
        Symbol::from("SYN"),
        2,
        vec![comp1, comp2],
        &formula,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let _ = actor.add_synthetic(synthetic);
}

#[rstest]
#[should_panic(expected = "Actor has not been registered")]
fn test_update_synthetic_panics_when_unregistered() {
    use std::str::FromStr;

    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::SyntheticInstrument,
    };

    let actor = TestDataActor::new(DataActorConfig::default());
    let comp1 = InstrumentId::from_str("BTC-USD.VENUE").unwrap();
    let comp2 = InstrumentId::from_str("ETH-USD.VENUE").unwrap();
    let formula = format!("({comp1} + {comp2}) / 2.0");
    let synthetic = SyntheticInstrument::new(
        Symbol::from("SYN"),
        2,
        vec![comp1, comp2],
        &formula,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let _ = actor.update_synthetic(synthetic);
}

#[rstest]
fn test_subscribe_signal_multi_word_name_matches_published_topic(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    // Multi-word / mixed-case names round-trip through the Python-compatible
    // title-case topic scheme (`data.Signal<TitleName>`), matching v1 behavior.
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_signal("hello world", None);
    drop(actor);

    let publisher = get_actor_unchecked::<TestDataActor>(&actor_id);
    publisher.publish_signal("hello world", "ok".to_string(), UnixNanos::default());
    // A differently-cased input produces a different title-cased topic and
    // therefore must not match the `hello world` subscription.
    publisher.publish_signal("unrelated", "skip".to_string(), UnixNanos::default());
    drop(publisher);

    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    assert_eq!(actor.received_signals.len(), 1);
    assert_eq!(actor.received_signals[0].name.as_str(), "hello world");
    assert_eq!(actor.received_signals[0].value, "ok");
}

#[rstest]
#[case("example", "1.5", 0)]
#[case("risk", "HIGH", 1_700_000_000_000_000_000)]
fn test_publish_signal_reaches_subscriber(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    #[case] name: &str,
    #[case] value: &str,
    #[case] ts_event: u64,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_signal(name, None);
    drop(actor);

    let publisher = get_actor_unchecked::<TestDataActor>(&actor_id);
    publisher.publish_signal(name, value.to_string(), UnixNanos::from(ts_event));
    drop(publisher);

    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    assert_eq!(actor.received_signals.len(), 1);
    let signal = &actor.received_signals[0];
    assert_eq!(signal.name.as_str(), name);
    assert_eq!(signal.value, value);
    if ts_event != 0 {
        assert_eq!(signal.ts_event, UnixNanos::from(ts_event));
    }
}

#[rstest]
fn test_subscribe_signal_wildcard_matches_all_names(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Empty name = subscribe to all signals
    actor.subscribe_signal("", None);
    drop(actor);

    let publisher = get_actor_unchecked::<TestDataActor>(&actor_id);
    publisher.publish_signal("alpha", "1".to_string(), UnixNanos::default());
    publisher.publish_signal("beta", "2".to_string(), UnixNanos::default());
    drop(publisher);

    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    assert_eq!(actor.received_signals.len(), 2);
    assert_eq!(actor.received_signals[0].name.as_str(), "alpha");
    assert_eq!(actor.received_signals[1].name.as_str(), "beta");
}

#[rstest]
fn test_unsubscribe_signal_stops_delivery(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_signal("alpha", None);
    drop(actor);

    let publisher = get_actor_unchecked::<TestDataActor>(&actor_id);
    publisher.publish_signal("alpha", "1".to_string(), UnixNanos::default());
    drop(publisher);

    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    assert_eq!(actor.received_signals.len(), 1);

    actor.unsubscribe_signal("alpha");
    drop(actor);

    let publisher = get_actor_unchecked::<TestDataActor>(&actor_id);
    publisher.publish_signal("alpha", "2".to_string(), UnixNanos::default());
    drop(publisher);

    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    assert_eq!(actor.received_signals.len(), 1);
}

#[rstest]
#[case(100, 10)]
#[case(1_000_000, 10)] // Above old u8 ceiling: locks in u32 widening
#[case(u32::MAX, 0)] // Saturated boundary
fn test_subscribe_signal_dispatches_in_priority_order(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    #[case] high_priority: u32,
    #[case] low_priority: u32,
) {
    use crate::msgbus::switchboard::get_signal_topic;

    set_data_cmd_sender(Arc::new(SyncDataCommandSender));
    *get_message_bus().borrow_mut() = MessageBus::default();

    let mut actor_high = TestDataActor::new(DataActorConfig {
        actor_id: Some(ActorId::new("ACTOR-HIGH")),
        ..DataActorConfig::default()
    });
    actor_high
        .register(trader_id, clock.clone(), cache.clone())
        .unwrap();
    let high_id = actor_high.actor_id().inner();
    register_actor(actor_high);

    let mut actor_low = TestDataActor::new(DataActorConfig {
        actor_id: Some(ActorId::new("ACTOR-LOW")),
        ..DataActorConfig::default()
    });
    actor_low.register(trader_id, clock, cache).unwrap();
    let low_id = actor_low.actor_id().inner();
    register_actor(actor_low);

    let mut high = get_actor_unchecked::<TestDataActor>(&high_id);
    high.start().unwrap();
    high.subscribe_signal("trigger", Some(high_priority));
    drop(high);

    let mut low = get_actor_unchecked::<TestDataActor>(&low_id);
    low.start().unwrap();
    low.subscribe_signal("trigger", Some(low_priority));
    drop(low);

    // Bus must dispatch the high-priority subscription first regardless of
    // registration order, including for priorities above the old u8 ceiling.
    let topic = get_signal_topic("trigger");
    let subs = get_message_bus().borrow_mut().matching_subscriptions(topic);
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0].priority, high_priority);
    assert_eq!(subs[1].priority, low_priority);

    // Both actors still receive the signal end-to-end.
    let publisher = get_actor_unchecked::<TestDataActor>(&high_id);
    publisher.publish_signal("trigger", "go".to_string(), UnixNanos::default());
    drop(publisher);

    let high = get_actor_unchecked::<TestDataActor>(&high_id);
    let low = get_actor_unchecked::<TestDataActor>(&low_id);
    assert_eq!(high.received_signals.len(), 1);
    assert_eq!(low.received_signals.len(), 1);
}

#[rstest]
fn test_subscribe_signal_resubscribe_does_not_update_priority(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    use crate::msgbus::switchboard::get_signal_topic;

    let actor_id = register_data_actor(clock, cache, trader_id);
    let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // First subscription wins; second is silently dropped (warn-only).
    actor.subscribe_signal("trigger", Some(10));
    actor.subscribe_signal("trigger", Some(100));
    drop(actor);

    let topic = get_signal_topic("trigger");
    let subs = get_message_bus().borrow_mut().matching_subscriptions(topic);
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].priority, 10);
}

#[rstest]
fn test_add_synthetic_stores_in_cache(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    use std::str::FromStr;

    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::SyntheticInstrument,
    };

    let actor_id = register_data_actor(clock, cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);

    let comp1 = InstrumentId::from_str("BTC-USD.VENUE").unwrap();
    let comp2 = InstrumentId::from_str("ETH-USD.VENUE").unwrap();
    let formula = format!("({comp1} + {comp2}) / 2.0");
    let synthetic = SyntheticInstrument::new(
        Symbol::from("SYN"),
        2,
        vec![comp1, comp2],
        &formula,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let synthetic_id = synthetic.id;

    actor.add_synthetic(synthetic.clone()).unwrap();

    assert!(cache.borrow().synthetic(&synthetic_id).is_some());

    // Adding again should error
    let err = actor.add_synthetic(synthetic).unwrap_err().to_string();
    assert!(err.contains("already exists"));
}

#[rstest]
fn test_update_synthetic_replaces_existing(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    use std::str::FromStr;

    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::SyntheticInstrument,
    };

    let actor_id = register_data_actor(clock, cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);

    let comp1 = InstrumentId::from_str("BTC-USD.VENUE").unwrap();
    let comp2 = InstrumentId::from_str("ETH-USD.VENUE").unwrap();
    let symbol = Symbol::from("SYN");
    let original_formula = format!("({comp1} + {comp2}) / 2.0");
    let synthetic = SyntheticInstrument::new(
        symbol,
        2,
        vec![comp1, comp2],
        &original_formula,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let synthetic_id = synthetic.id;

    // update before add should error
    let err = actor
        .update_synthetic(synthetic.clone())
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not exist"));

    actor.add_synthetic(synthetic).unwrap();

    let new_formula = format!("{comp1} + {comp2}");
    let updated = SyntheticInstrument::new(
        symbol,
        2,
        vec![comp1, comp2],
        &new_formula,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    actor.update_synthetic(updated).unwrap();

    let guard = cache.borrow();
    let stored = guard.synthetic(&synthetic_id).unwrap();
    assert_eq!(stored.formula, new_formula);
}
