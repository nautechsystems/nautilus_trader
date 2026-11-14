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

use super::*;

#[rstest]
#[case::close(MarketStatusAction::Close, false)]
#[case::halt(MarketStatusAction::Halt, false)]
#[case::pause(MarketStatusAction::Pause, false)]
#[case::suspend(MarketStatusAction::Suspend, false)]
#[case::trading(MarketStatusAction::Trading, true)]
fn test_trailing_stop_does_not_trail_while_market_not_open(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[case] action: MarketStatusAction,
    #[case] is_open: bool,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));

    let config = OrderMatchingEngineConfig {
        support_gtd_orders: true,
        ..Default::default()
    };

    let mut engine = get_order_matching_engine(
        instrument_eth_usdt.clone(),
        None,
        Some(cache),
        Some(AccountType::Margin),
        Some(config),
    );

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1000.25", 1));

    let trailing_id = ClientOrderId::from("O-19700101-000000-001-001-1");
    let mut trailing = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(trailing_id)
        .submit(true)
        .build();
    engine.process_order(&mut trailing, account_id);

    let gtd_id = ClientOrderId::from("O-19700101-000000-001-001-2");
    let mut gtd_limit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Buy)
        .price(Price::from("990.00"))
        .quantity(Quantity::from("1.000"))
        .time_in_force(TimeInForce::Gtd)
        .expire_time(UnixNanos::from(2))
        .client_order_id(gtd_id)
        .submit(true)
        .build();
    engine.process_order(&mut gtd_limit, account_id);
    clear_order_event_handler_messages(&handler);

    // A higher bid while the market is not open, then command-settlement iterate on reopen
    engine.process_status(action);
    engine.process_quote_tick(&quote("1001.25", "1001.50", 2));

    // GTD expiry still runs while the market is not open
    let pre_reopen_events = get_order_event_handler_messages(&handler);
    assert!(
        pre_reopen_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Expired(e) if e.client_order_id == gtd_id))
    );

    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    engine.process_quote_tick(&quote("1000.50", "1000.75", 3));

    let events = get_order_event_handler_messages(&handler);

    let triggers: Vec<Price> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(e) if e.client_order_id == trailing_id => e.trigger_price,
            _ => None,
        })
        .collect();

    let fills: Vec<Price> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(e) if e.client_order_id == trailing_id => Some(e.last_px),
            _ => None,
        })
        .collect();

    if is_open {
        // Trails up to 1001.25 - 0.50 and the reopen bid triggers it
        assert_eq!(triggers, vec![Price::from("1000.75")]);
        assert_eq!(fills, vec![Price::from("1000.50")]);
    } else {
        // Trails from the reopen bid only: 1000.50 - 0.50, above the initial 999.50
        assert_eq!(triggers, vec![Price::from("1000.00")]);
        assert!(fills.is_empty(), "trailing stop filled: {fills:?}");
    }
}

#[rstest]
fn test_trailing_stop_reopen_requires_fresh_channel_data(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));

    let config = OrderMatchingEngineConfig {
        trade_execution: false,
        ..Default::default()
    };

    let mut engine = get_order_matching_engine(
        instrument_eth_usdt.clone(),
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        Some(config),
    );

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_eth_usdt.id(),
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let trade = |price: &str, ts: u64| {
        TradeTick::new(
            instrument_eth_usdt.id(),
            Price::from(price),
            Quantity::from("1.000"),
            AggressorSide::Buy,
            TradeId::from("T-1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let mark = |price: &str, ts: u64| {
        MarkPriceUpdate::new(
            instrument_eth_usdt.id(),
            Price::from(price),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1000.25", 1));
    engine.process_trade_tick(&trade("1000.00", 1));
    engine.process_mark_price(&mark("1000.00", 1));

    // Unactivated BidAsk order with activation_price touched only while closed
    let bid_ask_id = ClientOrderId::from("O-19700101-000000-001-001-1");
    let mut bid_ask_order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .activation_price(Price::from("1001.00"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(bid_ask_id)
        .submit(true)
        .build();
    engine.process_order(&mut bid_ask_order, account_id);

    // Activated LastPrice and MarkPrice trailing stops
    let last_id = ClientOrderId::from("O-19700101-000000-001-001-2");
    let mut last_order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::LastPrice)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(last_id)
        .submit(true)
        .build();
    engine.process_order(&mut last_order, account_id);

    let mark_id = ClientOrderId::from("O-19700101-000000-001-001-3");
    let mut mark_order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::MarkPrice)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(mark_id)
        .submit(true)
        .build();
    engine.process_order(&mut mark_order, account_id);

    let last_or_bid_ask_id = ClientOrderId::from("O-19700101-000000-001-001-4");
    let mut last_or_bid_ask_order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_eth_usdt.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::LastOrBidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(last_or_bid_ask_id)
        .submit(true)
        .build();
    engine.process_order(&mut last_or_bid_ask_order, account_id);
    clear_order_event_handler_messages(&handler);

    engine.process_status(MarketStatusAction::Close);
    engine.process_quote_tick(&quote("1001.25", "1001.50", 2));
    engine.process_trade_tick(&trade("1001.25", 2));
    engine.process_mark_price(&mark("1001.25", 2));

    // Reopen + iterate + unrelated channel updates must not activate or trail from closure prices
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);
    engine.process_mark_price(&mark("1000.50", 4));

    let events = get_order_event_handler_messages(&handler);

    let updated_ids: Vec<(ClientOrderId, Option<Price>)> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(e) => Some((e.client_order_id, e.trigger_price)),
            _ => None,
        })
        .collect();

    assert_eq!(updated_ids, vec![(mark_id, Some(Price::from("1000.00")))]);

    // Quote update refreshes BidAsk and LastOrBidAsk (not LastPrice), trailing from 1001.00
    clear_order_event_handler_messages(&handler);
    engine.process_quote_tick(&quote("1001.00", "1001.25", 5));

    let events = get_order_event_handler_messages(&handler);

    let updated_ids: Vec<(ClientOrderId, Option<Price>)> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(e) => Some((e.client_order_id, e.trigger_price)),
            _ => None,
        })
        .collect();

    assert_eq!(
        updated_ids,
        vec![
            (last_or_bid_ask_id, Some(Price::from("1000.50"))),
            (bid_ask_id, Some(Price::from("1000.50"))),
        ]
    );
}

#[rstest]
fn test_trailing_stop_follows_trade_only_l1_data_while_open(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let buyer_trade = |price: &str, ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1.000"),
            AggressorSide::Buy,
            TradeId::from("T-1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_trade_tick(&buyer_trade("1000.00", 1));
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Buy, account_id);
    clear_order_event_handler_messages(&handler);

    engine.process_trade_tick(&buyer_trade("999.00", 2));
    engine.process_trade_tick(&buyer_trade("998.00", 3));

    // Each trade iteration sees the ask the previous trade left, so 999.00 + 0.50
    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("999.50")]
    );
}

#[rstest]
fn test_trailing_stop_follows_trades_after_one_sided_depth_while_open(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let buyer_trade = |price: &str, ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1.000"),
            AggressorSide::Buy,
            TradeId::from("T-1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&QuoteTick::new(
        instrument_id,
        Price::from("1000.00"),
        Price::from("1000.25"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    ));
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Buy, account_id);
    clear_order_event_handler_messages(&handler);

    // A bid-only depth clears the ask, then trades set it again
    engine
        .process_order_book_depth(&depth_snapshot(instrument_id, Some("999.00"), None, 2, 2))
        .unwrap();
    engine.process_trade_tick(&buyer_trade("999.50", 3));
    engine.process_trade_tick(&buyer_trade("999.25", 4));
    engine.process_trade_tick(&buyer_trade("999.00", 5));

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("1000.00"), Price::from("999.75")]
    );
}

#[rstest]
#[case::updated_after_reopen(true, vec![Price::from("110.50")])]
#[case::left_from_closure(false, vec![])]
fn test_trailing_stop_follows_l2_level_updated_after_reopen(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[case] update_level: bool,
    #[case] expected_triggers: Vec<Price>,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine_l2(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let delta = |action: BookAction, side: OrderSide, price: &str, size: &str| {
        OrderBookDeltaTestBuilder::new(instrument_id)
            .book_action(action)
            .book_order(BookOrder::new(
                side,
                Price::from(price),
                Quantity::from(size),
                0,
            ))
            .build()
    };

    engine
        .process_order_book_delta(&delta(BookAction::Add, OrderSide::Buy, "99.00", "10.000"))
        .unwrap();
    engine
        .process_order_book_delta(&delta(BookAction::Add, OrderSide::Sell, "115.00", "10.000"))
        .unwrap();
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Buy, account_id);
    clear_order_event_handler_messages(&handler);

    engine.process_status(MarketStatusAction::Close);
    engine
        .process_order_book_delta(&delta(BookAction::Add, OrderSide::Sell, "100.00", "10.000"))
        .unwrap();
    engine
        .process_order_book_delta(&delta(BookAction::Add, OrderSide::Sell, "110.00", "10.000"))
        .unwrap();
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(1), AggressorSide::NoAggressor);
    if update_level {
        engine
            .process_order_book_delta(&delta(
                BookAction::Update,
                OrderSide::Sell,
                "110.00",
                "5.000",
            ))
            .unwrap();
    }

    // Deleting the closure best ask exposes 110.00, followed only once updated
    engine
        .process_order_book_delta(&delta(
            BookAction::Delete,
            OrderSide::Sell,
            "100.00",
            "10.000",
        ))
        .unwrap();

    assert_eq!(get_updated_trigger_prices(&handler), expected_triggers);
}

#[rstest]
#[case::new_price("1000.50", "1000.75", "1000.00")]
#[case::closure_price("1001.25", "1001.50", "1000.75")]
fn test_trailing_stop_ignores_out_of_order_l1_depth_after_reopen(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[case] reopen_bid: &str,
    #[case] reopen_ask: &str,
    #[case] expected_trigger: &str,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1000.25", 1));
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Sell, account_id);
    clear_order_event_handler_messages(&handler);

    engine.process_status(MarketStatusAction::Close);
    engine.process_quote_tick(&quote("1001.25", "1001.50", 3));
    engine.process_status(MarketStatusAction::Trading);

    // The book skips a depth older than the closure quote, which stays in the book
    engine
        .process_order_book_depth(&depth_snapshot(
            instrument_id,
            Some("1000.50"),
            Some("1000.75"),
            2,
            4,
        ))
        .unwrap();
    assert!(get_updated_trigger_prices(&handler).is_empty());

    // A quote after the reopen is followed, even at the closure price
    engine.process_quote_tick(&quote(reopen_bid, reopen_ask, 5));

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from(expected_trigger)]
    );
}

#[rstest]
fn test_trailing_stop_follows_trades_after_reopen(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let trade = |price: &str, aggressor_side: AggressorSide, ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1.000"),
            aggressor_side,
            TradeId::from("T-1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1000.25", 1));
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Sell, account_id);
    clear_order_event_handler_messages(&handler);

    engine.process_status(MarketStatusAction::Close);
    engine.process_quote_tick(&quote("1001.25", "1001.50", 2));
    engine.process_status(MarketStatusAction::Trading);

    // A buyer trade below the closure bid leaves that bid in place
    engine.process_trade_tick(&trade("1000.50", AggressorSide::Buy, 3));
    assert!(get_updated_trigger_prices(&handler).is_empty());

    // A seller trade sets the bid it hit: 1000.25 - 0.50
    engine.process_trade_tick(&trade("1000.25", AggressorSide::Sell, 4));
    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("999.75")]
    );
}

#[rstest]
fn test_trailing_stop_reopen_follows_repeated_depth_trade(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(OrderType::TrailingStopMarket, OrderType::TrailingStopLimit)] order_type: OrderType,
    #[values(true, false)] trade_execution: bool,
    #[values(false, true)] no_aggressor: bool,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));

    let config = OrderMatchingEngineConfig {
        trade_execution,
        ..Default::default()
    };

    let mut engine = build_caching_engine(
        instrument_eth_usdt,
        book_type,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        Some(config),
    );

    let delta = |side, price: &str, id| {
        OrderBookDeltaTestBuilder::new(instrument_id)
            .book_action(BookAction::Update)
            .book_order(BookOrder::new(
                side,
                Price::from(price),
                Quantity::from("10.000"),
                id,
            ))
            .build()
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, "1000.00", 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, "1000.25", 2))
        .unwrap();
    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .limit_offset(dec!(0.25))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);

    let (bid, ask, price, aggressor, trigger, limit) = match side {
        OrderSide::Sell => (
            "1001.25",
            "1001.50",
            "1001.25",
            AggressorSide::Sell,
            "1000.75",
            "1001.00",
        ),
        OrderSide::Buy => (
            "998.50",
            "998.75",
            "998.75",
            AggressorSide::Buy,
            "999.25",
            "999.00",
        ),
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, bid, 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, ask, 2))
        .unwrap();
    engine.process_status(MarketStatusAction::Trading);

    let trade = TradeTick::new(
        instrument_id,
        Price::from(price),
        Quantity::from("1.000"),
        if no_aggressor {
            AggressorSide::NoAggressor
        } else {
            aggressor
        },
        TradeId::from("T-REOPEN"),
        UnixNanos::from(3),
        UnixNanos::from(3),
    );

    engine.process_trade_tick(&trade);

    let events = get_order_event_handler_messages(&handler);

    let updates: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(e) => {
                Some((e.client_order_id, e.quantity, e.trigger_price, e.price))
            }
            _ => None,
        })
        .collect();

    let expected_price = (order_type == OrderType::TrailingStopLimit).then(|| Price::from(limit));
    assert_eq!(
        updates,
        vec![(
            order.client_order_id(),
            Quantity::from("1.000"),
            Some(Price::from(trigger)),
            expected_price
        )]
    );
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .trigger_price(),
        Some(Price::from(trigger))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_)))
    );
}

#[rstest]
#[case::accepted((BookAction::Update, "7.000", true, "7.000"))]
#[case::ignored_zero_add((BookAction::Add, "0.000", false, "10.000"))]
fn test_trailing_stop_reopen_tracks_accepted_delta(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[values(false, true)] batch: bool,
    #[values(false, true)] resolve_side: bool,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[case] update: (BookAction, &str, bool, &str),
) {
    let (action, size, is_accepted, expected_size) = update;
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = build_caching_engine(
        instrument_eth_usdt,
        book_type,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let delta = |side, price: &str, id| {
        OrderBookDeltaTestBuilder::new(instrument_id)
            .book_action(BookAction::Update)
            .book_order(BookOrder::new(
                side,
                Price::from(price),
                Quantity::from("10.000"),
                id,
            ))
            .build()
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, "1000.00", 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, "1000.25", 2))
        .unwrap();
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, side, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);

    let (bid, ask, book_side, price, trigger) = match side {
        OrderSide::Buy => ("998.50", "998.75", OrderSide::Sell, "998.75", "999.25"),
        OrderSide::Sell => ("1001.25", "1001.50", OrderSide::Buy, "1001.25", "1000.75"),
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, bid, 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, ask, 2))
        .unwrap();
    let id = engine
        .get_book()
        .get_orders_at_level(Price::from(price), side)[0]
        .order_id;
    engine.process_status(MarketStatusAction::Trading);
    let mut update = delta(book_side, price, id);
    update.action = action;
    if resolve_side {
        update.order.side = None;
    }

    update.order.size = Quantity::from(size);

    if batch {
        engine
            .process_order_book_deltas(&OrderBookDeltas::new(instrument_id, vec![update]))
            .unwrap();
    } else {
        engine.process_order_book_delta(&update).unwrap();
    }

    let expected_triggers = if is_accepted {
        vec![Price::from(trigger)]
    } else {
        vec![]
    };

    assert_eq!(get_updated_trigger_prices(&handler), expected_triggers);

    let size = match side {
        OrderSide::Buy => engine.get_book().best_ask_size(),
        OrderSide::Sell => engine.get_book().best_bid_size(),
    };

    assert_eq!(size, Some(Quantity::from(expected_size)));
}

#[rstest]
#[case::last(TriggerType::LastPrice, false)]
#[case::last_activation(TriggerType::LastPrice, true)]
#[case::bid_ask(TriggerType::BidAsk, true)]
#[case::last_or_bid_ask(TriggerType::LastOrBidAsk, true)]
fn test_trailing_stop_reopen_waits_for_trade_after_maker_fill(
    #[values(1, 2)] maker_count: usize,
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(OrderType::TrailingStopMarket, OrderType::TrailingStopLimit)] order_type: OrderType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[case] trigger_type: TriggerType,
    #[case] has_activation_price: bool,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        None,
    );

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1002.00", 1));

    let (closed_bid, closed_ask, activation, trade_price, trigger, limit, aggressor) = match side {
        OrderSide::Buy => (
            "999.00",
            "1000.00",
            "1001.50",
            "1000.75",
            "1001.25",
            "1001.00",
            AggressorSide::Buy,
        ),
        OrderSide::Sell => (
            "1002.00",
            "1003.00",
            "1000.50",
            "1001.25",
            "1000.75",
            "1001.00",
            AggressorSide::Sell,
        ),
    };

    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from("1.000"))
        .trigger_type(trigger_type)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .limit_offset(dec!(0.25))
        .submit(true);

    if has_activation_price {
        builder.activation_price(Price::from(activation));
    }

    let mut trailing = builder.build();
    engine.process_order(&mut trailing, account_id);

    let maker_prices = match side {
        OrderSide::Buy => ["1001.00", "1000.75"],
        OrderSide::Sell => ["1001.00", "1001.25"],
    };

    let mut expected_fills = Vec::new();

    for (index, price) in maker_prices.into_iter().take(maker_count).enumerate() {
        let mut maker = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from(format!("O-MAKER-{index}")))
            .side(side)
            .quantity(Quantity::from("1.000"))
            .price(Price::from(price))
            .submit(true)
            .build();
        engine.process_order(&mut maker, account_id);
        expected_fills.push((
            maker.client_order_id(),
            Price::from(price),
            Quantity::from("1.000"),
            LiquiditySide::Maker,
        ));
    }

    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);
    engine.process_quote_tick(&quote(closed_bid, closed_ask, 2));
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);

    let events = get_order_event_handler_messages(&handler);

    let fills: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(e) => {
                Some((e.client_order_id, e.last_px, e.last_qty, e.liquidity_side))
            }
            _ => None,
        })
        .collect();

    assert_eq!(fills, expected_fills);
    assert_eq!(get_updated_trigger_prices(&handler), vec![]);
    let cached = cache.borrow();
    let order = cached.order(&trailing.client_order_id()).unwrap();
    assert!(!trailing_stop_is_activated(&order));
    assert_eq!(order.trigger_price(), None);
    drop(order);
    drop(cached);

    clear_order_event_handler_messages(&handler);
    engine.process_trade_tick(&TradeTick::new(
        instrument_id,
        Price::from(trade_price),
        Quantity::from("1.000"),
        aggressor,
        TradeId::from("T-FRESH"),
        UnixNanos::from(4),
        UnixNanos::from(4),
    ));
    let cached = cache.borrow();
    let order = cached.order(&trailing.client_order_id()).unwrap();
    assert!(trailing_stop_is_activated(&order));
    assert_eq!(order.trigger_price(), Some(Price::from(trigger)));

    if order_type == OrderType::TrailingStopLimit {
        assert_eq!(order.price(), Some(Price::from(limit)));
    }
}

#[rstest]
#[case::ambiguous_then_delete(true, vec![], "10.000")]
#[case::update_then_ambiguous(false, vec![Price::from("1000.75")], "9.000")]
fn test_trailing_stop_reopen_resolves_batch_updates_in_order(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[case] initially_ambiguous: bool,
    #[case] expected_triggers: Vec<Price>,
    #[case] expected_size: &str,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = build_caching_engine(
        instrument_eth_usdt,
        book_type,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let delta = |action, side, price: &str, size: &str, id| {
        OrderBookDeltaTestBuilder::new(instrument_id)
            .book_action(action)
            .book_order(BookOrder::new(
                side,
                Price::from(price),
                Quantity::from(size),
                id,
            ))
            .build()
    };

    engine
        .process_order_book_delta(&delta(
            BookAction::Add,
            OrderSide::Buy,
            "1000.00",
            "10.000",
            1,
        ))
        .unwrap();
    engine
        .process_order_book_delta(&delta(
            BookAction::Add,
            OrderSide::Sell,
            "1000.25",
            "10.000",
            2,
        ))
        .unwrap();
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Sell, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);
    engine
        .process_order_book_delta(&delta(
            BookAction::Update,
            OrderSide::Buy,
            "1001.25",
            "10.000",
            1,
        ))
        .unwrap();
    engine
        .process_order_book_delta(&delta(
            BookAction::Delete,
            OrderSide::Sell,
            "1000.25",
            "10.000",
            2,
        ))
        .unwrap();
    engine
        .process_order_book_delta(&delta(
            BookAction::Update,
            OrderSide::Sell,
            "1002.00",
            "10.000",
            2,
        ))
        .unwrap();
    let id = engine.get_book().bids(None).next().unwrap().get_orders()[0].order_id;
    if initially_ambiguous {
        engine
            .process_order_book_delta(&delta(
                BookAction::Add,
                OrderSide::Sell,
                "1001.25",
                "10.000",
                id,
            ))
            .unwrap();
    }

    engine.process_status(MarketStatusAction::Trading);
    let mut update = delta(BookAction::Update, OrderSide::Buy, "1001.25", "9.000", id);
    update.order.side = None;

    let second_action = if initially_ambiguous {
        BookAction::Delete
    } else {
        BookAction::Add
    };

    let second = delta(second_action, OrderSide::Sell, "1001.25", "10.000", id);
    engine
        .process_order_book_deltas(&OrderBookDeltas::new(instrument_id, vec![update, second]))
        .unwrap();

    assert_eq!(get_updated_trigger_prices(&handler), expected_triggers);
    assert_eq!(
        engine.get_book().best_bid_size(),
        Some(Quantity::from(expected_size))
    );
    assert_eq!(
        engine.get_book().best_ask_price(),
        Some(Price::from(if initially_ambiguous {
            "1002.00"
        } else {
            "1001.25"
        }))
    );
}

#[rstest]
fn test_trailing_stop_reopen_submission_waits_for_its_channel(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(OrderType::TrailingStopMarket, OrderType::TrailingStopLimit)] order_type: OrderType,
    #[values(
        TriggerType::BidAsk,
        TriggerType::LastPrice,
        TriggerType::MarkPrice,
        TriggerType::LastOrBidAsk
    )]
    trigger_type: TriggerType,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        None,
    );

    let quote = |price: &str, ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from(price),
            Price::from(price),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let trade = |price: &str, ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1.000"),
            AggressorSide::Sell,
            TradeId::from("T-CHANNEL"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let mark = |price: &str, ts: u64| {
        MarkPriceUpdate::new(
            instrument_id,
            Price::from(price),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", 1));
    engine.process_trade_tick(&trade("1000.00", 1));
    engine.process_mark_price(&mark("1000.00", 1));
    engine.process_status(MarketStatusAction::Close);

    let create_order = |id: &str| {
        OrderTestBuilder::new(order_type)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.000"))
            .trigger_type(trigger_type)
            .trailing_offset(dec!(0.50))
            .trailing_offset_type(TrailingOffsetType::Price)
            .limit_offset(dec!(0.25))
            .submit(true)
            .build()
    };

    let mut order = create_order("O-19700101-000000-001-001-1");
    engine.process_order(&mut order, account_id);
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Rejected
    );
    assert!(!trailing_stop_is_activated(
        &cache.borrow().order(&order.client_order_id()).unwrap()
    ));
    clear_order_event_handler_messages(&handler);

    for (price, ts, expected_trigger) in [("1001.25", 2, "1000.75"), ("1002.25", 4, "1001.75")] {
        engine.process_quote_tick(&quote(price, ts));
        engine.process_trade_tick(&trade(price, ts));
        engine.process_mark_price(&mark(price, ts));
        engine.process_status(MarketStatusAction::Trading);

        if ts == 2 {
            order = create_order("O-19700101-000000-001-001-2");
            engine.process_order(&mut order, account_id);
        }

        engine.iterate(UnixNanos::from(ts), AggressorSide::NoAggressor);
        assert_eq!(get_updated_trigger_prices(&handler), vec![]);

        match trigger_type {
            TriggerType::LastPrice => engine.process_trade_tick(&trade(price, ts + 1)),
            TriggerType::MarkPrice => engine.process_mark_price(&mark(price, ts + 1)),
            _ => engine.process_quote_tick(&quote(price, ts + 1)),
        }

        let cached = cache.borrow();
        let current = cached.order(&order.client_order_id()).unwrap();
        assert!(trailing_stop_is_activated(&current));
        assert_eq!(current.trigger_price(), Some(Price::from(expected_trigger)));
        assert_eq!(
            get_updated_trigger_prices(&handler),
            vec![Price::from(expected_trigger)]
        );
        clear_order_event_handler_messages(&handler);
        engine.process_status(MarketStatusAction::Close);
    }
}

#[rstest]
#[case::last("LAST")]
#[case::mid("MID")]
#[case::quotes("BID_ASK")]
fn test_trailing_stop_reopen_accepts_bar_prices(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[case] source: &str,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let send_bars = |engine: &mut CachingEngine, price: &str, ts: u64| {
        let sources: &[&str] = if source == "BID_ASK" {
            &["BID", "ASK"]
        } else {
            &[source]
        };

        for source in sources {
            engine.process_bar(&Bar::new(
                BarType::from(format!("{instrument_id}-1-MINUTE-{source}-EXTERNAL").as_str()),
                Price::from(price),
                Price::from(price),
                Price::from(price),
                Price::from(price),
                Quantity::from("40.000"),
                UnixNanos::from(ts),
                UnixNanos::from(ts),
            ));
        }
    };

    send_bars(&mut engine, "1000.00", 1);
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, OrderSide::Sell, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);
    send_bars(&mut engine, "1001.25", 2);
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert_eq!(get_updated_trigger_prices(&handler), vec![]);
    send_bars(&mut engine, "1001.25", 3);

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("1000.75")]
    );
}

#[rstest]
fn test_trailing_stop_reset_clears_reopen_freshness(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        Some(OrderMatchingEngineConfig {
            reject_stop_orders: false,
            ..Default::default()
        }),
    );

    let _handler = order_event_handler_with_cache(Rc::clone(&cache));
    engine.process_status(MarketStatusAction::Close);
    engine.process_status(MarketStatusAction::Trading);
    engine.reset();
    engine.process_quote_tick(&QuoteTick::new(
        instrument_id,
        Price::from("1001.00"),
        Price::from("1001.25"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    ));
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .activation_price(Price::from("1000.50"))
        .trigger_type(TriggerType::LastPrice)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let cached = cache.borrow();
    let current = cached.order(&order.client_order_id()).unwrap();
    assert!(trailing_stop_is_activated(&current));
    assert_eq!(current.activation_price(), Some(Price::from("1000.50")));
    assert_eq!(current.trigger_price(), None);
}

#[rstest]
fn test_trailing_stop_reopen_accepts_depth_snapshot(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(BookType::L1_MBP, BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = build_caching_engine(
        instrument_eth_usdt,
        book_type,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );
    engine
        .process_order_book_depth(&depth_snapshot(
            instrument_id,
            Some("1000.00"),
            Some("1000.25"),
            1,
            1,
        ))
        .unwrap();
    submit_bid_ask_trailing_stop(&mut engine, instrument_id, side, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);

    let (bid, ask, trigger) = match side {
        OrderSide::Buy => ("998.50", "998.75", "999.25"),
        OrderSide::Sell => ("1001.25", "1001.50", "1000.75"),
    };

    engine
        .process_order_book_depth(&depth_snapshot(instrument_id, Some(bid), Some(ask), 2, 2))
        .unwrap();
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert_eq!(get_updated_trigger_prices(&handler), vec![]);
    engine
        .process_order_book_depth(&depth_snapshot(instrument_id, Some(bid), Some(ask), 3, 3))
        .unwrap();

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from(trigger)]
    );
}

#[rstest]
fn test_trailing_stop_reopen_uses_zero_volume_bar_price(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let bar = |price: &str, ts: u64| {
        Bar::new(
            BarType::from(format!("{instrument_id}-1-MINUTE-LAST-EXTERNAL").as_str()),
            Price::from(price),
            Price::from(price),
            Price::from(price),
            Price::from(price),
            Quantity::from("0.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_bar(&bar("1000.00", 1));
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::LastPrice)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);
    engine.process_bar(&bar("1001.25", 2));
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert_eq!(get_updated_trigger_prices(&handler), vec![]);
    engine.process_bar(&bar("1001.50", 3));
    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("1001.00")]
    );
}

#[rstest]
fn test_trailing_stop_reopen_follows_one_sided_bar_leg(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));
    let mut engine = get_order_matching_engine(
        instrument_eth_usdt,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        None,
    );

    let quote = |bid: &str, ask: &str, ts: u64| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine.process_quote_tick(&quote("1000.00", "1000.25", 1));
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);

    let (bid, ask, bid_high, bid_low, ask_high, ask_low, bid_volume, ask_volume, trigger) =
        match side {
            OrderSide::Sell => (
                "1001.25", "1001.50", "1001.25", "1000.00", "1001.50", "1000.25", "40.000",
                "0.001", "1000.75",
            ),
            OrderSide::Buy => (
                "998.50", "998.75", "1000.00", "998.50", "1000.25", "998.75", "0.001", "40.000",
                "999.25",
            ),
        };

    engine.process_quote_tick(&quote(bid, ask, 2));
    engine.process_status(MarketStatusAction::Trading);
    for (source, open, high, low, volume) in [
        ("BID", bid, bid_high, bid_low, bid_volume),
        ("ASK", ask, ask_high, ask_low, ask_volume),
    ] {
        engine.process_bar(&Bar::new(
            BarType::from(format!("{instrument_id}-1-MINUTE-{source}-EXTERNAL").as_str()),
            Price::from(open),
            Price::from(high),
            Price::from(low),
            Price::from(open),
            Quantity::from(volume),
            UnixNanos::from(3),
            UnixNanos::from(3),
        ));
    }

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from(trigger)]
    );
    let cached = cache.borrow();
    let current = cached.order(&order.client_order_id()).unwrap();
    assert_eq!(current.status(), OrderStatus::Filled);
    assert_eq!(current.filled_qty(), Quantity::from("1.000"));

    let fills: Vec<_> = get_order_event_handler_messages(&handler)
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(e) => Some(e.last_px),
            _ => None,
        })
        .collect();

    assert_eq!(fills, vec![Price::from(trigger)]);
}

#[rstest]
fn test_trailing_stop_reopen_after_expiration_revision_waits_for_quote(account_id: AccountId) {
    let mut binary = binary_option();
    let instrument = InstrumentAny::BinaryOption(binary.clone());
    let instrument_id = instrument.id();
    let expiration = instrument.expiration_ns().unwrap().as_u64();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    clock.borrow_mut().set_time(UnixNanos::from(
        instrument.activation_ns().unwrap().as_u64() + 1,
    ));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));

    let mut engine = get_order_matching_engine(
        instrument,
        Some(Rc::clone(&clock)),
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        Some(OrderMatchingEngineConfig {
            reject_stop_orders: false,
            ..Default::default()
        }),
    );

    let quote = |ts| {
        QuoteTick::new(
            instrument_id,
            Price::from("0.450"),
            Price::from("0.550"),
            Quantity::from("10.00"),
            Quantity::from("10.00"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    clock.borrow_mut().set_time(UnixNanos::from(expiration + 1));
    engine.process_quote_tick(&quote(expiration + 1));
    assert_eq!(engine.market_status, MarketStatus::Closed);
    binary.expiration_ns = UnixNanos::from(expiration + 1000);
    engine
        .update_instrument(InstrumentAny::BinaryOption(binary))
        .unwrap();
    assert_eq!(engine.market_status, MarketStatus::Open);
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.00"))
        .activation_price(Price::from("0.420"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.010))
        .trailing_offset_type(TrailingOffsetType::Price)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    assert!(!trailing_stop_is_activated(
        &cache.borrow().order(&order.client_order_id()).unwrap()
    ));
    clear_order_event_handler_messages(&handler);
    engine.process_quote_tick(&quote(expiration + 2));

    assert_eq!(
        get_updated_trigger_prices(&handler),
        vec![Price::from("0.440")]
    );
    assert!(trailing_stop_is_activated(
        &cache.borrow().order(&order.client_order_id()).unwrap()
    ));
}

#[rstest]
fn test_trailing_stop_reopen_uses_last_with_stale_depth(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(BookType::L2_MBP, BookType::L3_MBO)] book_type: BookType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(OrderType::TrailingStopMarket, OrderType::TrailingStopLimit)] order_type: OrderType,
) {
    let instrument_id = instrument_eth_usdt.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(Rc::clone(&cache));

    let config = OrderMatchingEngineConfig {
        trade_execution: false,
        ..Default::default()
    };

    let mut engine = build_caching_engine(
        instrument_eth_usdt,
        book_type,
        None,
        Some(Rc::clone(&cache)),
        Some(AccountType::Margin),
        Some(config),
    );

    let delta = |side, price: &str, id| {
        OrderBookDeltaTestBuilder::new(instrument_id)
            .book_action(BookAction::Update)
            .book_order(BookOrder::new(
                side,
                Price::from(price),
                Quantity::from("10.000"),
                id,
            ))
            .build()
    };

    let trade = |price: &str, ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from(price),
            Quantity::from("1.000"),
            AggressorSide::NoAggressor,
            TradeId::from("T-LAST"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, "1000.00", 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, "1000.25", 2))
        .unwrap();
    engine.process_trade_tick(&trade("1000.00", 1));
    let combined_id = ClientOrderId::from("O-COMBINED");
    let quote_id = ClientOrderId::from("O-QUOTE");

    for (trigger_type, id) in [
        (TriggerType::LastOrBidAsk, combined_id),
        (TriggerType::BidAsk, quote_id),
    ] {
        let mut order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument_id)
            .client_order_id(id)
            .side(side)
            .quantity(Quantity::from("1.000"))
            .trigger_type(trigger_type)
            .trailing_offset(dec!(0.50))
            .trailing_offset_type(TrailingOffsetType::Price)
            .limit_offset(dec!(0.25))
            .submit(true)
            .build();
        engine.process_order(&mut order, account_id);
    }

    clear_order_event_handler_messages(&handler);
    engine.process_status(MarketStatusAction::Close);
    engine
        .process_order_book_delta(&OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    let (bid, ask, last, trigger, limit, quote_trigger) = match side {
        OrderSide::Sell => (
            "1001.25", "1001.50", "1001.00", "1000.50", "1000.75", "999.50",
        ),
        OrderSide::Buy => ("998.50", "998.75", "999.00", "999.50", "999.25", "1000.75"),
    };

    engine
        .process_order_book_delta(&delta(OrderSide::Buy, bid, 1))
        .unwrap();
    engine
        .process_order_book_delta(&delta(OrderSide::Sell, ask, 2))
        .unwrap();
    engine.process_status(MarketStatusAction::Trading);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert_eq!(get_order_event_handler_messages(&handler).len(), 0);

    engine.process_trade_tick(&trade(last, 3));
    let events = get_order_event_handler_messages(&handler);

    let OrderEventAny::Updated(updated) = &events[0] else {
        panic!("Expected trailing update, was {events:?}");
    };

    assert_eq!(events.len(), 1);
    assert_eq!(updated.client_order_id, combined_id);
    assert_eq!(updated.quantity, Quantity::from("1.000"));
    assert_eq!(updated.trigger_price, Some(Price::from(trigger)));
    assert_eq!(
        updated.price,
        (order_type == OrderType::TrailingStopLimit).then(|| Price::from(limit))
    );
    assert_eq!(
        cache.borrow().order(&quote_id).unwrap().trigger_price(),
        Some(Price::from(quote_trigger))
    );
    assert_eq!(engine.get_core().bid, Some(Price::from(bid)));
    assert_eq!(engine.get_core().ask, Some(Price::from(ask)));
}

fn submit_bid_ask_trailing_stop(
    engine: &mut CachingEngine,
    instrument_id: InstrumentId,
    side: OrderSide,
    account_id: AccountId,
) {
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from("1.000"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(dec!(0.50))
        .trailing_offset_type(TrailingOffsetType::Price)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
}

fn get_updated_trigger_prices(
    handler: &TypedIntoMessageSavingHandler<OrderEventAny>,
) -> Vec<Price> {
    get_order_event_handler_messages(handler)
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(e) => e.trigger_price,
            _ => None,
        })
        .collect()
}

fn depth_snapshot(
    instrument_id: InstrumentId,
    bid: Option<&str>,
    ask: Option<&str>,
    ts_event: u64,
    ts_init: u64,
) -> OrderBookDepth {
    let mut bids = [BookOrder::default(); DEPTH10_LEN];
    let mut asks = [BookOrder::default(); DEPTH10_LEN];

    if let Some(bid) = bid {
        bids[0] = BookOrder::new(
            OrderSide::Buy,
            Price::from(bid),
            Quantity::from("10.000"),
            1,
        );
    }

    if let Some(ask) = ask {
        asks[0] = BookOrder::new(
            OrderSide::Sell,
            Price::from(ask),
            Quantity::from("10.000"),
            2,
        );
    }

    OrderBookDepth::new(
        instrument_id,
        bids,
        asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        0,
        UnixNanos::from(ts_event),
        UnixNanos::from(ts_init),
    )
}
