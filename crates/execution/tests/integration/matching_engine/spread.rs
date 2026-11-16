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

use std::cell::Cell;

use nautilus_execution::models::fee::{FeeModel, FeeModelHandle, PerContractFeeModel};
#[cfg(feature = "high-precision")]
use nautilus_model::instruments::{CryptoFuture, CryptoFuturesSpread};
use nautilus_model::{
    accounts::Account,
    enums::PriceType,
    events::account::stubs::cash_account_state_million_usd,
    instruments::{
        FuturesSpread,
        stubs::{
            crypto_option_btc_deribit, crypto_option_spread_btc_deribit, option_contract_appl,
            option_spread,
        },
    },
    orders::stubs::TestOrderEventStubs,
};

use super::*;

#[cfg(feature = "high-precision")]
#[rstest]
#[case::representable(1_000_000_000_000_u128, true)]
#[case::product(4_000_000_000_000_u128, false)]
#[case::parent(8_000_000_000_000_u128, false)]
fn test_spread_preserves_native_quantity_ticks(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] whole: u128,
    #[case] accountable: bool,
    #[values(TimeInForce::Gtc, TimeInForce::Fok)] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let increment = Quantity::from("0.0000000000000001");
    let quantity = Quantity::from_raw_checked(whole * 10_u128.pow(16) + 1, 16).unwrap();
    let leg_quantity = Quantity::from_raw_checked(quantity.raw() * 2, 16).unwrap();

    let legs = ["ESM4", "NQM4"].map(|symbol| {
        InstrumentAny::CryptoFuture(
            CryptoFuture::builder()
                .instrument_id(InstrumentId::from(format!("{symbol}.XCME")))
                .raw_symbol(Symbol::from(symbol))
                .underlying(Currency::BTC())
                .quote_currency(Currency::USD())
                .settlement_currency(Currency::USD())
                .is_inverse(false)
                .activation_ns(0.into())
                .expiration_ns(100_000_000_000.into())
                .price_precision(2)
                .size_precision(16)
                .price_increment(Price::from("0.01"))
                .size_increment(increment)
                .lot_size(increment)
                .ts_event(0.into())
                .ts_init(0.into())
                .build()
                .unwrap(),
        )
    });

    let [leg_a, leg_b] = legs;
    let spread = InstrumentAny::CryptoFuturesSpread(
        CryptoFuturesSpread::builder()
            .instrument_id(InstrumentId::from("(2)ESM4___((1))NQM4.XCME"))
            .raw_symbol(Symbol::from("RATIO"))
            .underlying(Currency::BTC())
            .quote_currency(Currency::USD())
            .settlement_currency(Currency::USD())
            .is_inverse(false)
            .strategy_type("SPREAD".into())
            .activation_ns(0.into())
            .expiration_ns(100_000_000_000.into())
            .price_precision(2)
            .size_precision(16)
            .price_increment(Price::from("0.01"))
            .size_increment(increment)
            .lot_size(increment)
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap(),
    );

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    assert_eq!(spread.try_normalize_qty(quantity).unwrap(), quantity);
    assert_eq!(leg_a.try_normalize_qty(leg_quantity).unwrap(), leg_quantity);
    assert_eq!(leg_b.try_normalize_qty(quantity).unwrap(), quantity);
    cache_leg_quote(&cache, leg_a.id(), "1.00", "1.00");
    cache_leg_quote(&cache, leg_b.id(), "1.00", "1.00");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&QuoteTick::new(
        spread.id(),
        Price::from("1.00"),
        Price::from("1.00"),
        quantity,
        quantity,
        UnixNanos::default(),
        UnixNanos::default(),
    ));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(quantity)
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);

    if !accountable {
        let order = cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .clone();
        assert_eq!(fills.len(), 0);
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.filled_qty().raw(), 0);
        assert_eq!(order.leaves_qty().raw(), quantity.raw());
        return;
    }

    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, expected)) in [
        (&leg_a, OrderSide::Buy, leg_quantity),
        (&leg_b, OrderSide::Sell, quantity),
        (&spread, OrderSide::Buy, quantity),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_qty.raw(), expected.raw());
        assert_eq!(fills[index].last_qty.precision, expected.precision);
        assert_eq!(fills[index].last_px, Price::from("1.00"));
    }

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty().raw(), quantity.raw());
}

#[rstest]
#[case::market(OrderType::Market, TimeInForce::Gtc)]
#[case::market_to_limit(OrderType::MarketToLimit, TimeInForce::Gtc)]
#[case::fok(OrderType::Market, TimeInForce::Fok)]
fn test_spread_leg_venue_id_collision_preserves_existing_order(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] order_type: OrderType,
    #[case] time_in_force: TimeInForce,
    #[values(0_usize, 1)] leg_position: usize,
) {
    let cache = spread_cache(account_id);
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    let (instrument, side, price) = [
        (&leg_a, OrderSide::Sell, "5200.25"),
        (&leg_b, OrderSide::Buy, "18130.75"),
    ][leg_position];
    let venue_order_id = VenueOrderId::from(format!("XCME-1-1-LEG-{leg_position}"));
    let mut existing = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(side)
        .price(Price::from(price))
        .quantity(Quantity::from(7))
        .client_order_id(ClientOrderId::from("O-EXISTING"))
        .submit(true)
        .build();
    existing
        .apply(TestOrderEventStubs::accepted(
            &existing,
            account_id,
            venue_order_id,
        ))
        .unwrap();
    cache
        .borrow_mut()
        .add_order(existing.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_venue_order_id(&existing.client_order_id(), &venue_order_id, false)
        .unwrap();
    assert_eq!(
        cache.borrow().client_order_id(&venue_order_id),
        Some(&existing.client_order_id())
    );
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let execution = Rc::new(RefCell::new(ExecutionEngine::new(
        clock.clone(),
        cache.clone(),
        None,
    )));

    let events = Rc::new(RefCell::new(Vec::new()));
    let saved = events.clone();
    let mut engine = get_order_matching_engine(
        spread.clone(),
        Some(clock),
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.set_event_handler(Rc::new(move |event| {
        execution.borrow_mut().process(&event);
        saved.borrow_mut().push(event);
    }));

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .client_order_id(ClientOrderId::from("O-SPREAD"))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    assert!(!cache.borrow().order_exists(&ClientOrderId::from(format!(
        "O-SPREAD-LEG-{}",
        instrument.id().symbol
    ))));
    engine.process_order(&mut order, account_id);

    let cache = cache.borrow();
    let original = cache.order(&existing.client_order_id()).unwrap();
    let parent = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(original.filled_qty(), Quantity::from(0));
    assert_eq!(original.status(), OrderStatus::Accepted);
    assert_eq!(original.instrument_id(), instrument.id());
    assert_eq!(original.order_side(), side);
    assert_eq!(original.quantity(), Quantity::from(7));
    assert_eq!(original.price(), Some(Price::from(price)));
    assert_eq!(original.venue_order_id(), Some(venue_order_id));
    assert_eq!(
        cache.client_order_id(&venue_order_id),
        Some(&existing.client_order_id())
    );
    assert_eq!(parent.status(), OrderStatus::Canceled);
    assert_eq!(parent.filled_qty(), Quantity::from(0));
    assert_eq!(parent.price(), None);
    assert_eq!(cache.positions_total_count(None, None, None, None, None), 0);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_) | OrderEventAny::Updated(_)))
            .count(),
        0
    );
}

#[rstest]
fn test_option_spread_fills_zero_bid_leg(#[from(spread_account_id)] account_id: AccountId) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let mut leg_a = option_contract_appl();
    leg_a.id = InstrumentId::from("AAPL-C150.OPRA");
    leg_a.raw_symbol = leg_a.id.symbol;
    leg_a.activation_ns = UnixNanos::default();
    let mut leg_b = leg_a.clone();
    leg_b.id = InstrumentId::from("AAPL-C155.OPRA");
    leg_b.raw_symbol = leg_b.id.symbol;
    leg_b.strike_price = Price::from("155.00");
    let mut spread = option_spread();
    spread.id = InstrumentId::from("(1)AAPL-C150___((1))AAPL-C155.OPRA");
    spread.activation_ns = UnixNanos::default();
    spread.underlying = leg_a.underlying;
    spread.multiplier = leg_a.multiplier;
    let leg_a = InstrumentAny::OptionContract(leg_a);
    let leg_b = InstrumentAny::OptionContract(leg_b);
    let spread = InstrumentAny::OptionSpread(spread);
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "1.00", "1.05");
    cache_leg_quote(&cache, leg_b.id(), "0.00", "0.05");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "0.95", "1.05"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, price)) in [
        (&leg_a, OrderSide::Buy, "1.05"),
        (&leg_b, OrderSide::Sell, "0.00"),
        (&spread, OrderSide::Buy, "1.05"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_px, Price::from(price));
        assert_eq!(fills[index].last_px.precision, instrument.price_precision());
        assert_eq!(fills[index].last_qty, Quantity::from(1));
    }

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from(1));
}

#[rstest]
fn test_spread_non_ascii_leg_id_fails_without_fills(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("LÉG.XCME", "LÉG");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))LÉG___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from(0));
}

#[rstest]
fn test_spread_fixed_tick_scheme_preserves_small_price_difference(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let mut leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let mut leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let mut spread = futures_spread_with_generic_id("(1)ESM4___((1))NQM4.XCME", "SPREAD");

    for leg in [&mut leg_a, &mut leg_b] {
        if let InstrumentAny::FuturesContract(instrument) = leg {
            instrument.price_precision = 9;
            instrument.price_increment = Price::from("0.000000001");
            instrument.tick_scheme = Some(Ustr::from("FIXED_PRECISION_9"));
        }
    }

    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.price_precision = 9;
        instrument.price_increment = Price::from("0.000000001");
    }

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(
        &cache,
        leg_a.id(),
        "1000000000.000000001",
        "1000000000.000000001",
    );
    cache_leg_quote(
        &cache,
        leg_b.id(),
        "1000000000.000000000",
        "1000000000.000000000",
    );
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "0.000000001", "0.000000001"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[0].last_px, Price::from("1000000000.000000001"));
    assert_eq!(fills[1].last_px, Price::from("1000000000.000000000"));
    assert_eq!(fills[2].last_px, Price::from("0.000000001"));
    assert_eq!(
        fills[0].last_px.as_decimal() - fills[1].last_px.as_decimal(),
        fills[2].last_px.as_decimal()
    );
}

#[rstest]
fn test_spread_market_fill_emits_combo_and_leg_fills(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let order_event_handler = order_event_handler_with_cache(cache.clone());

    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "((1))ESM4___(1)NQM4");

    cache.borrow_mut().add_instrument(leg_a.clone()).unwrap();
    cache.borrow_mut().add_instrument(leg_b.clone()).unwrap();
    cache.borrow_mut().add_instrument(spread.clone()).unwrap();

    let ts = UnixNanos::from(1_715_248_800_000_000_000u64);
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    clock.borrow_mut().set_time(ts);
    cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            leg_a.id(),
            Price::from("5200.25"),
            Price::from("5200.50"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ))
        .unwrap();
    cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            leg_b.id(),
            Price::from("18130.50"),
            Price::from("18130.75"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ))
        .unwrap();

    let mut engine = get_order_matching_engine(
        spread.clone(),
        Some(clock),
        Some(cache),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&QuoteTick::new(
        spread.id(),
        Price::from("12930.25"),
        Price::from("12930.50"),
        Quantity::from(10),
        Quantity::from(10),
        ts,
        ts,
    ));

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .client_order_id(ClientOrderId::from("O-SPREAD"))
        .submit(true)
        .build();

    clear_order_event_handler_messages(&order_event_handler);
    engine.process_order(&mut order, account_id);

    let fills: Vec<OrderFilled> = get_order_event_handler_messages(&order_event_handler)
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    // Leg fills book the accounting, so they precede the spread fill
    assert_eq!(
        fills.len(),
        3,
        "events={:?}",
        order_event_handler.get_messages()
    );
    assert_eq!(
        fills
            .iter()
            .map(|fill| fill.trade_id)
            .collect::<AHashSet<_>>()
            .len(),
        3
    );

    for fill in &fills {
        assert_eq!(fill.account_id, account_id);
        assert_eq!(fill.order_type, OrderType::Market);
        assert_eq!(fill.currency, Currency::USD());
        assert_eq!(fill.commission, Some(Money::from("0 USD")));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    }

    assert_eq!(fills[2].instrument_id, spread.id());
    assert_eq!(fills[2].last_qty, Quantity::from(5));
    assert_eq!(fills[2].last_px, Price::from("12930.50"));

    assert_eq!(fills[0].instrument_id, leg_a.id());
    assert_eq!(
        fills[0].client_order_id,
        ClientOrderId::from("O-SPREAD-LEG-ESM4")
    );
    assert_eq!(
        fills[0].venue_order_id,
        VenueOrderId::from("XCME-1-1-LEG-0")
    );
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[0].last_qty, Quantity::from(5));
    assert_eq!(fills[0].last_px, Price::from("5200.25"));

    assert_eq!(fills[1].instrument_id, leg_b.id());
    assert_eq!(
        fills[1].client_order_id,
        ClientOrderId::from("O-SPREAD-LEG-NQM4")
    );
    assert_eq!(
        fills[1].venue_order_id,
        VenueOrderId::from("XCME-1-1-LEG-1")
    );
    assert_eq!(fills[1].order_side, OrderSide::Buy);
    assert_eq!(fills[1].last_qty, Quantity::from(5));
    assert_eq!(fills[1].last_px, Price::from("18130.75"));
}

#[rstest]
fn test_spread_market_fill_sweeping_l2_levels_emits_leg_fills_per_fill(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let order_event_handler = order_event_handler_with_cache(cache.clone());

    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "((1))ESM4___(1)NQM4");

    cache.borrow_mut().add_instrument(leg_a.clone()).unwrap();
    cache.borrow_mut().add_instrument(leg_b.clone()).unwrap();
    cache.borrow_mut().add_instrument(spread.clone()).unwrap();

    let ts = UnixNanos::from(1_715_248_800_000_000_000u64);
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    clock.borrow_mut().set_time(ts);
    cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            leg_a.id(),
            Price::from("5200.25"),
            Price::from("5200.50"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ))
        .unwrap();
    cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            leg_b.id(),
            Price::from("18130.50"),
            Price::from("18130.75"),
            Quantity::from(10),
            Quantity::from(10),
            ts,
            ts,
        ))
        .unwrap();

    let mut engine = get_order_matching_engine_l2(
        spread.clone(),
        Some(clock),
        Some(cache),
        Some(AccountType::Margin),
        None,
    );

    let ask_level_1 = OrderBookDeltaTestBuilder::new(spread.id())
        .book_action(BookAction::Add)
        .book_order(BookOrder::new(
            OrderSide::Sell,
            Price::from("12930.50"),
            Quantity::from(3),
            1,
        ))
        .build();
    engine.process_order_book_delta(&ask_level_1).unwrap();

    let ask_level_2 = OrderBookDeltaTestBuilder::new(spread.id())
        .book_action(BookAction::Add)
        .book_order(BookOrder::new(
            OrderSide::Sell,
            Price::from("12931.00"),
            Quantity::from(7),
            2,
        ))
        .build();
    engine.process_order_book_delta(&ask_level_2).unwrap();

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .client_order_id(ClientOrderId::from("O-SPREAD"))
        .submit(true)
        .build();

    clear_order_event_handler_messages(&order_event_handler);
    engine.process_order(&mut order, account_id);

    let fills: Vec<OrderFilled> = get_order_event_handler_messages(&order_event_handler)
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    // Each swept level emits its leg fills followed by the spread fill
    assert_eq!(fills.len(), 6);
    assert_eq!(
        fills
            .iter()
            .map(|fill| fill.trade_id)
            .collect::<AHashSet<_>>()
            .len(),
        6
    );
    assert_eq!(fills[0].client_order_id, fills[3].client_order_id);
    assert_eq!(fills[1].client_order_id, fills[4].client_order_id);

    assert_eq!(fills[2].instrument_id, spread.id());
    assert_eq!(fills[2].last_qty, Quantity::from(3));
    assert_eq!(fills[2].last_px, Price::from("12930.50"));

    assert_eq!(fills[0].instrument_id, leg_a.id());
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[0].last_qty, Quantity::from(3));
    assert_eq!(fills[0].last_px, Price::from("5200.25"));

    assert_eq!(fills[1].instrument_id, leg_b.id());
    assert_eq!(fills[1].order_side, OrderSide::Buy);
    assert_eq!(fills[1].last_qty, Quantity::from(3));
    assert_eq!(fills[1].last_px, Price::from("18130.75"));

    assert_eq!(fills[5].instrument_id, spread.id());
    assert_eq!(fills[5].last_qty, Quantity::from(2));
    assert_eq!(fills[5].last_px, Price::from("12931.00"));

    assert_eq!(fills[3].instrument_id, leg_a.id());
    assert_eq!(fills[3].order_side, OrderSide::Sell);
    assert_eq!(fills[3].last_qty, Quantity::from(2));
    assert_eq!(fills[3].last_px, Price::from("5200.25"));

    assert_eq!(fills[4].instrument_id, leg_b.id());
    assert_eq!(fills[4].order_side, OrderSide::Buy);
    assert_eq!(fills[4].last_qty, Quantity::from(2));
    assert_eq!(fills[4].last_px, Price::from("18131.25"));

    let sum_fill_qty = |instrument_id: InstrumentId| {
        fills
            .iter()
            .filter(|fill| fill.instrument_id == instrument_id)
            .fold(Quantity::from(0), |acc, fill| acc + fill.last_qty)
    };

    assert_eq!(sum_fill_qty(spread.id()), Quantity::from(5));
    assert_eq!(sum_fill_qty(leg_a.id()), Quantity::from(5));
    assert_eq!(sum_fill_qty(leg_b.id()), Quantity::from(5));
}

#[rstest]
#[case::buy(OrderSide::Buy, OrderSide::Sell, OrderSide::Buy, "43991.75")]
#[case::sell(OrderSide::Sell, OrderSide::Buy, OrderSide::Sell, "43990.50")]
fn test_spread_fill_preserves_signed_nonunit_ratios(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] side: OrderSide,
    #[case] leg_a_side: OrderSide,
    #[case] leg_b_side: OrderSide,
    #[case] spread_price: &str,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((2))ESM4___(3)NQM4.XCME", "RATIO");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "43990.50", "43991.75"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(side)
        .quantity(Quantity::from(4))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[0].instrument_id, leg_a.id());
    assert_eq!(fills[0].order_side, leg_a_side);
    assert_eq!(fills[0].last_qty, Quantity::from(8));
    assert_eq!(fills[1].instrument_id, leg_b.id());
    assert_eq!(fills[1].order_side, leg_b_side);
    assert_eq!(fills[1].last_qty, Quantity::from(12));
    assert_eq!(fills[2].instrument_id, spread.id());
    assert_eq!(fills[2].last_qty, Quantity::from(4));
    assert_eq!(fills[2].last_px, Price::from(spread_price));
    assert_eq!(
        -dec!(2) * fills[0].last_px.as_decimal() + dec!(3) * fills[1].last_px.as_decimal(),
        fills[2].last_px.as_decimal(),
    );
}

#[rstest]
#[case::fixed_once(
    FeeModelAny::Fixed(FixedFeeModel::new(Money::from("2.50 USD"), Some(true)).unwrap()),
    TimeInForce::Gtc,
    ["2.50 USD", "2.50 USD", "5.00 USD", "0 USD", "0 USD", "0 USD"],
)]
#[case::per_contract(
    FeeModelAny::PerContract(PerContractFeeModel::new(Money::from("1.25 USD")).unwrap()),
    TimeInForce::Gtc,
    ["3.75 USD", "3.75 USD", "7.50 USD", "2.50 USD", "2.50 USD", "5.00 USD"],
)]
#[case::maker_taker(
    FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.04), dec!(0.08))),
    TimeInForce::Gtc,
    ["1248.06 USD", "4351.38 USD", "5599.44 USD", "832.04 USD", "2901.00 USD", "3733.04 USD"],
)]
fn test_spread_partial_fills_charge_leg_fees_once(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] fee_model: FeeModelAny,
    #[case] time_in_force: TimeInForce,
    #[case] commissions: [&str; 6],
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    let matching = OrderMatchingEngine::new(
        spread.clone(),
        1,
        FillModelHandle::default(),
        fee_model.into(),
        BookType::L2_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );

    let mut engine = CachingEngine {
        engine: matching,
        cache,
    };

    for (price, qty, id) in [("12930.50", 3, 1), ("12931.00", 7, 2)] {
        let delta = OrderBookDeltaTestBuilder::new(spread.id())
            .book_action(BookAction::Add)
            .book_order(BookOrder::new(
                OrderSide::Sell,
                Price::from(price),
                Quantity::from(qty),
                id,
            ))
            .build();
        engine.process_order_book_delta(&delta).unwrap();
    }

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .time_in_force(time_in_force)
        .quantity(Quantity::from(5))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), commissions.len());

    for (fill, commission) in fills.iter().zip(commissions) {
        assert_eq!(fill.commission, Some(Money::from(commission)));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    }

    assert_eq!(fills[2].last_qty, Quantity::from(3));
    assert_eq!(fills[5].last_qty, Quantity::from(2));
}

#[rstest]
#[case::missing_instrument(
    "((1))ESM4___(1)NQM4.XCME",
    false,
    true,
    "12930.50",
    OrderType::Market,
    TimeInForce::Gtc
)]
#[case::missing_quote(
    "((1))ESM4___(1)NQM4.XCME",
    true,
    false,
    "12930.50",
    OrderType::Market,
    TimeInForce::Gtc
)]
#[case::native_id(
    "ES-NQ.XCME",
    true,
    true,
    "12930.50",
    OrderType::Market,
    TimeInForce::Gtc
)]
#[case::duplicate_legs(
    "((1))ESM4___(1)ESM4.XCME",
    true,
    true,
    "0.25",
    OrderType::Market,
    TimeInForce::Gtc
)]
#[case::invalid_price_grid(
    "((1))ESM4___(1)NQM4.XCME",
    true,
    true,
    "12930.60",
    OrderType::Market,
    TimeInForce::Gtc
)]
#[case::initial_market_to_limit(
    "((1))ESM4___(1)NQM4.XCME",
    true,
    false,
    "12930.50",
    OrderType::MarketToLimit,
    TimeInForce::Gtc
)]
#[case::initial_market_to_limit_ioc(
    "((1))ESM4___(1)NQM4.XCME",
    true,
    false,
    "12930.50",
    OrderType::MarketToLimit,
    TimeInForce::Ioc
)]
fn test_spread_preparation_failure_emits_no_fills(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] spread_id: &str,
    #[case] leg_b_present: bool,
    #[case] leg_b_quoted: bool,
    #[case] spread_price: &str,
    #[case] order_type: OrderType,
    #[case] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let mut spread = futures_spread_with_generic_id(spread_id, "SPREAD");
    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.price_increment = Price::from("0.01");
    }

    cache.borrow_mut().add_instrument(leg_a.clone()).unwrap();
    cache.borrow_mut().add_instrument(spread.clone()).unwrap();
    if leg_b_present {
        cache.borrow_mut().add_instrument(leg_b.clone()).unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    if leg_b_quoted {
        cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    }

    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), spread_price, spread_price));
    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(3))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    assert_eq!(order.price(), None);
    engine.process_order(&mut order, account_id);

    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(
        events
            .get_messages()
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Updated(_)))
            .count(),
        0
    );
    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from(0));
    assert_eq!(order.price(), None);
}

#[rstest]
#[case::lot_grid(1, "0.2")]
#[case::precision(0, "1")]
fn test_crypto_option_spread_rejects_unrepresentable_leg_qty(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] leg_precision: u8,
    #[case] leg_increment: &str,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = crypto_option_btc_deribit(
        2,
        leg_precision,
        Price::from("0.01"),
        Quantity::from(leg_increment),
    );
    let mut leg_b = crypto_option_btc_deribit(2, 1, Price::from("0.01"), Quantity::from("0.1"));
    leg_b.id = InstrumentId::from("BTC-13JAN23-17000-P.DERIBIT");
    leg_b.raw_symbol = Symbol::from("BTC-13JAN23-17000-P");
    leg_b.strike_price = Price::from("17000.000");
    let mut spread = crypto_option_spread_btc_deribit();
    spread.id = InstrumentId::from("((1))BTC-13JAN23-16000-P___(1)BTC-13JAN23-17000-P.DERIBIT");
    spread.activation_ns = UnixNanos::default();
    let leg_a = InstrumentAny::CryptoOption(leg_a);
    let leg_b = InstrumentAny::CryptoOption(leg_b);
    let spread = InstrumentAny::CryptoOptionSpread(spread);
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "1.00", "1.10");
    cache_leg_quote(&cache, leg_b.id(), "2.00", "2.10");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&QuoteTick::new(
        spread.id(),
        Price::from("0.9000"),
        Price::from("1.1000"),
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    ));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.1"))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
}

#[rstest]
fn test_spread_limit_retries_after_missing_leg_quote_arrives(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");

    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );

    let mut quote = spread_quote(spread.id(), "12930.25", "12930.50");
    quote.ask_size = Quantity::from(3);
    engine.process_quote_tick(&quote);
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .price(Price::from("12930.50"))
        .quantity(Quantity::from(3))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    let fills_before_quote = saved_fills(&events);
    let status_before_quote = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .status();
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    engine.process_quote_tick(&quote);

    let fills = saved_fills(&events);
    assert_eq!(fills_before_quote, Vec::new());
    assert_eq!(status_before_quote, OrderStatus::Accepted);
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[2].last_qty, Quantity::from(3));
    assert_eq!(fills[2].last_px, Price::from("12930.50"));
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Filled
    );
}

#[rstest]
fn test_spread_rejects_liquidity_consumption_before_matching(
    #[from(spread_account_id)] account_id: AccountId,
    #[values(OrderType::Market, OrderType::Limit, OrderType::MarketToLimit)] order_type: OrderType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok)] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        Some(OrderMatchingEngineConfig {
            liquidity_consumption: true,
            ..Default::default()
        }),
    );

    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(spread.id())
        .side(side)
        .quantity(Quantity::from(3))
        .time_in_force(time_in_force)
        .submit(true);

    if order_type == OrderType::Limit {
        builder.price(Price::from(if side == OrderSide::Buy {
            "12930.50"
        } else {
            "12930.25"
        }));
    }

    let mut order = builder.build();
    engine.process_order(&mut order, account_id);
    let messages = events.get_messages();
    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(messages.len(), 1);

    let OrderEventAny::Rejected(rejected) = &messages[0] else {
        panic!("Expected rejection before matching")
    };

    assert_eq!(
        rejected.reason.as_str(),
        "Simulated spread orders do not support liquidity_consumption=true"
    );
    assert_eq!(rejected.client_order_id, order.client_order_id());
    assert_eq!(rejected.account_id, account_id);
    assert_eq!(order.status(), OrderStatus::Rejected);
    assert_eq!(order.filled_qty(), Quantity::from(0));
    assert_eq!(order.leaves_qty(), Quantity::from(3));
    assert_eq!(
        cache
            .borrow()
            .positions_total_count(None, None, None, None, None),
        0
    );
}

#[rstest]
fn test_fok_spread_cancels_multiple_executions_before_any_fill(
    #[from(spread_account_id)] account_id: AccountId,
    #[values(OrderType::Market, OrderType::Limit)] order_type: OrderType,
    #[values("12930.51", "12930.75")] second_price: &str,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let mut spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.price_increment = Price::from("0.01");
    }

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    let mut engine = get_order_matching_engine_l2(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );

    for (price, qty, id) in [("12930.50", 3, 1), (second_price, 2, 2)] {
        let delta = OrderBookDeltaTestBuilder::new(spread.id())
            .book_action(BookAction::Add)
            .book_order(BookOrder::new(
                OrderSide::Sell,
                Price::from(price),
                Quantity::from(qty),
                id,
            ))
            .build();
        engine.process_order_book_delta(&delta).unwrap();
    }

    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(spread.id())
        .client_order_id(ClientOrderId::from("O-FOK-INVALID"))
        .side(OrderSide::Buy)
        .time_in_force(TimeInForce::Fok)
        .quantity(Quantity::from(5))
        .submit(true);

    if order_type == OrderType::Limit {
        builder.price(Price::from(second_price));
    }

    let mut order = builder.build();
    engine.process_order(&mut order, account_id);

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from(0));

    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(spread.id())
        .client_order_id(ClientOrderId::from("O-FOK-VALID"))
        .side(OrderSide::Buy)
        .time_in_force(TimeInForce::Fok)
        .quantity(Quantity::from(3))
        .submit(true);

    if order_type == OrderType::Limit {
        builder.price(Price::from("12930.50"));
    }

    let mut valid_order = builder.build();
    engine.process_order(&mut valid_order, account_id);

    let fills = saved_fills(&events);
    let valid_order = cache
        .borrow()
        .order(&valid_order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[0].instrument_id, leg_a.id());
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[0].last_qty, Quantity::from(3));
    assert_eq!(fills[0].last_px, Price::from("5200.25"));
    assert_eq!(fills[1].instrument_id, leg_b.id());
    assert_eq!(fills[1].order_side, OrderSide::Buy);
    assert_eq!(fills[1].last_qty, Quantity::from(3));
    assert_eq!(fills[1].last_px, Price::from("18130.75"));
    assert_eq!(fills[2].instrument_id, spread.id());
    assert_eq!(fills[2].client_order_id, valid_order.client_order_id());
    assert_eq!(fills[2].last_qty, Quantity::from(3));
    assert_eq!(fills[2].last_px, Price::from("12930.50"));
    assert_eq!(valid_order.status(), OrderStatus::Filled);
    assert_eq!(valid_order.filled_qty(), Quantity::from(3));
}

#[rstest]
fn test_fok_spread_cancels_single_nonexecutable_candidate(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine_l2(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    let delta = OrderBookDeltaTestBuilder::new(spread.id())
        .book_action(BookAction::Add)
        .book_order(BookOrder::new(
            OrderSide::Sell,
            Price::from("12930.51"),
            Quantity::from(5),
            1,
        ))
        .build();
    engine.process_order_book_delta(&delta).unwrap();
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .time_in_force(TimeInForce::Fok)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from(0));
    assert_eq!(order.leaves_qty(), Quantity::from(5));
}

#[rstest]
fn test_spread_allocates_residual_across_both_leg_price_grids(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((2))ESM4___(3)NQM4.XCME", "RATIO");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "100.00", "100.00");
    cache_leg_quote(&cache, leg_b.id(), "200.00", "200.00");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "400.25", "400.25"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(2))
        .submit(true)
        .build();

    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), 3);
    assert_eq!(fills[0].instrument_id, leg_a.id());
    assert_eq!(fills[0].last_qty, Quantity::from(4));
    assert_eq!(fills[0].last_px, Price::from("100.25"));
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[1].instrument_id, leg_b.id());
    assert_eq!(fills[1].last_qty, Quantity::from(6));
    assert_eq!(fills[1].last_px, Price::from("200.25"));
    assert_eq!(fills[1].order_side, OrderSide::Buy);
    assert_eq!(fills[2].instrument_id, spread.id());
    assert_eq!(fills[2].last_qty, Quantity::from(2));
    assert_eq!(fills[2].last_px, Price::from("400.25"));
    assert_eq!(
        -dec!(2) * fills[0].last_px.as_decimal() + dec!(3) * fills[1].last_px.as_decimal(),
        fills[2].last_px.as_decimal(),
    );
}

#[rstest]
fn test_option_spread_fees_use_each_leg_underlying_price_context(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let mut leg_a = option_contract_appl();
    leg_a.id = InstrumentId::from("AAPL-C150.OPRA");
    leg_a.raw_symbol = leg_a.id.symbol;
    leg_a.multiplier = Quantity::from(100);
    leg_a.activation_ns = UnixNanos::default();
    let mut leg_b = leg_a.clone();
    leg_b.id = InstrumentId::from("AAPL-C155.OPRA");
    leg_b.raw_symbol = leg_b.id.symbol;
    leg_b.strike_price = Price::from("155.00");
    let mut spread = option_spread();
    spread.id = InstrumentId::from("(1)AAPL-C150___((2))AAPL-C155.OPRA");
    spread.activation_ns = UnixNanos::default();
    spread.underlying = leg_a.underlying;
    spread.multiplier = leg_a.multiplier;
    let leg_a = InstrumentAny::OptionContract(leg_a);
    let leg_b = InstrumentAny::OptionContract(leg_b);
    let spread = InstrumentAny::OptionSpread(spread);
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "10.00", "10.00");
    cache_leg_quote(&cache, leg_b.id(), "1.00", "1.00");
    cache_leg_quote(&cache, InstrumentId::from("AAPL.OPRA"), "150.00", "150.00");
    let fee_model = CappedOptionFeeModel::new(dec!(0.001), dec!(0.002), None).unwrap();

    let matching = OrderMatchingEngine::new(
        spread.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::CappedOption(fee_model).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );

    let mut engine = CachingEngine {
        engine: matching,
        cache,
    };

    engine.process_quote_tick(&spread_quote(spread.id(), "8.00", "8.00"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(3))
        .submit(true)
        .build();

    engine.process_order(&mut order, account_id);

    let fills = saved_fills(&events);
    assert_eq!(fills.len(), 3);

    for (fill, instrument_id, quantity, price, commission, side) in [
        (&fills[0], leg_a.id(), 3, "10.00", "90 USD", OrderSide::Buy),
        (&fills[1], leg_b.id(), 6, "1.00", "75 USD", OrderSide::Sell),
        (&fills[2], spread.id(), 3, "8.00", "165 USD", OrderSide::Buy),
    ] {
        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.last_qty, Quantity::from(quantity));
        assert_eq!(fill.last_px, Price::from(price));
        assert_eq!(fill.commission, Some(Money::from(commission)));
        assert_eq!(fill.currency, Currency::USD());
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    }
}

#[rstest]
fn test_spread_leg_client_id_collision_preserves_existing_order(
    #[from(spread_account_id)] account_id: AccountId,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let existing = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(leg_b.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(7))
        .price(Price::from("18140.00"))
        .client_order_id(ClientOrderId::from("O-SPREAD-LEG-ESM4"))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(existing.clone(), None, None, false)
        .unwrap();
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(3))
        .client_order_id(ClientOrderId::from("O-SPREAD"))
        .submit(true)
        .build();

    engine.process_order(&mut order, account_id);

    let cache = cache.borrow();
    let original = cache.order(&existing.client_order_id()).unwrap();
    let parent = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(parent.status(), OrderStatus::Canceled);
    assert_eq!(parent.filled_qty(), Quantity::from(0));
    assert_eq!(original.client_order_id(), existing.client_order_id());
    assert_eq!(original.instrument_id(), leg_b.id());
    assert_eq!(original.order_side(), OrderSide::Sell);
    assert_eq!(original.price(), Some(Price::from("18140.00")));
    assert_eq!(original.quantity(), Quantity::from(7));
    assert_eq!(original.filled_qty(), Quantity::from(0));
    assert_eq!(original.status(), OrderStatus::Submitted);
}

#[rstest]
fn test_market_to_limit_spread_defers_update_until_legs_are_prepared(
    #[from(spread_account_id)] account_id: AccountId,
    #[values(TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok)] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let mut spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.price_increment = Price::from("0.01");
    }

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    let mut engine = get_order_matching_engine_l2(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );

    let level = |action, price| {
        OrderBookDeltaTestBuilder::new(spread.id())
            .book_action(action)
            .book_order(BookOrder::new(
                OrderSide::Sell,
                Price::from(price),
                Quantity::from(5),
                1,
            ))
            .build()
    };

    engine
        .process_order_book_delta(&level(BookAction::Add, "12930.51"))
        .unwrap();
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .time_in_force(time_in_force)
        .client_order_id(ClientOrderId::from("O-MTL-INVALID"))
        .submit(true)
        .build();

    engine.process_order(&mut order, account_id);

    let invalid_events = events.get_messages();
    let invalid_order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    engine
        .process_order_book_delta(&level(BookAction::Delete, "12930.51"))
        .unwrap();
    engine
        .process_order_book_delta(&level(BookAction::Add, "12930.50"))
        .unwrap();
    events.clear();
    let mut valid_order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .time_in_force(time_in_force)
        .client_order_id(ClientOrderId::from("O-MTL-VALID"))
        .submit(true)
        .build();

    engine.process_order(&mut valid_order, account_id);

    let effects_count = invalid_events
        .iter()
        .filter(|event| matches!(event, OrderEventAny::Updated(_) | OrderEventAny::Filled(_)))
        .count();
    let valid_effects: Vec<_> = events
        .get_messages()
        .into_iter()
        .filter(|event| matches!(event, OrderEventAny::Updated(_) | OrderEventAny::Filled(_)))
        .collect();
    let fills = saved_fills(&events);
    let valid_order = cache
        .borrow()
        .order(&valid_order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(effects_count, 0);
    assert_eq!(invalid_order.status(), OrderStatus::Canceled);
    assert_eq!(invalid_order.filled_qty(), Quantity::from(0));
    assert_eq!(invalid_order.price(), None);
    assert_eq!(valid_effects.len(), 4);

    let OrderEventAny::Updated(updated) = &valid_effects[0] else {
        panic!("Expected price update before fills")
    };

    assert_eq!(updated.quantity, Quantity::from(5));
    assert_eq!(updated.price, Some(Price::from("12930.50")));
    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, price)) in [
        (&leg_a, OrderSide::Sell, "5200.25"),
        (&leg_b, OrderSide::Buy, "18130.75"),
        (&spread, OrderSide::Buy, "12930.50"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_qty, Quantity::from(5));
        assert_eq!(fills[index].last_px, Price::from(price));
    }

    assert_eq!(valid_order.status(), OrderStatus::Filled);
    assert_eq!(valid_order.filled_qty(), Quantity::from(5));
    assert_eq!(valid_order.price(), Some(Price::from("12930.50")));
}

#[rstest]
#[case::gtc(TimeInForce::Gtc)]
#[case::ioc(TimeInForce::Ioc)]
fn test_market_to_limit_spread_retries_resting_remainder_after_leg_preparation_failure(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let mut spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.price_increment = Price::from("0.01");
    }

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    let mut quote = spread_quote(spread.id(), "12930.25", "12930.50");
    quote.ask_size = Quantity::from(1);
    engine.process_quote_tick(&quote);
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(3))
        .time_in_force(time_in_force)
        .client_order_id(ClientOrderId::from("O-MTL-RESTING"))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let initial_fills = saved_fills(&events);
    let initial_order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(initial_fills.len(), 3);
    assert_eq!(initial_fills[2].last_qty, Quantity::from(1));
    assert_eq!(initial_fills[2].last_px, Price::from("12930.50"));
    assert_eq!(initial_order.status(), OrderStatus::Accepted);
    assert_eq!(initial_order.filled_qty(), Quantity::from(1));
    assert_eq!(initial_order.price(), Some(Price::from("12930.50")));

    // The model's book preserves price improvement for the resting remainder
    engine.set_fill_model(FillModelAny::BestPrice(BestPriceFillModel::default()).into());
    events.clear();
    let mut quote = spread_quote(spread.id(), "12930.25", "12930.49");
    quote.ask_size = Quantity::from(2);
    engine.process_quote_tick(&quote);

    let resting_order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(
        resting_order.status(),
        if time_in_force == TimeInForce::Gtc {
            OrderStatus::Accepted
        } else {
            OrderStatus::Canceled
        },
    );
    assert_eq!(resting_order.filled_qty(), Quantity::from(1));
    assert_eq!(resting_order.leaves_qty(), Quantity::from(2));
    assert_eq!(resting_order.price(), Some(Price::from("12930.50")));

    quote.ask_price = Price::from("12930.50");
    engine.process_quote_tick(&quote);

    let fills = saved_fills(&events);
    let completed_order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    if time_in_force == TimeInForce::Ioc {
        assert_eq!(fills, Vec::new());
        assert_eq!(completed_order.status(), OrderStatus::Canceled);
        assert_eq!(completed_order.filled_qty(), Quantity::from(1));
        assert_eq!(completed_order.leaves_qty(), Quantity::from(2));
        assert_eq!(completed_order.price(), Some(Price::from("12930.50")));
        return;
    }

    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, price)) in [
        (&leg_a, OrderSide::Sell, "5200.25"),
        (&leg_b, OrderSide::Buy, "18130.75"),
        (&spread, OrderSide::Buy, "12930.50"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_qty, Quantity::from(2));
        assert_eq!(fills[index].last_px, Price::from(price));
        assert_eq!(fills[index].liquidity_side, LiquiditySide::Maker);
    }

    assert_eq!(
        initial_fills
            .iter()
            .chain(&fills)
            .map(|fill| fill.trade_id)
            .collect::<AHashSet<_>>()
            .len(),
        6
    );
    assert_eq!(completed_order.status(), OrderStatus::Filled);
    assert_eq!(completed_order.filled_qty(), Quantity::from(3));
    assert_eq!(completed_order.leaves_qty(), Quantity::from(0));
    assert_eq!(completed_order.price(), Some(Price::from("12930.50")));
}

#[rstest]
fn test_spread_cash_conversion_rate_is_required_only_for_calculated_account(
    #[from(spread_account_id)] account_id: AccountId,
    #[values(true, false)] calculate_account_state: bool,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let events = order_event_handler_with_cache(cache.clone());
    let mut state = cash_account_state_million_usd("2500 USD", "0 USD", "2500 USD");
    state.account_id = account_id;
    cache
        .borrow_mut()
        .add_account(CashAccount::new(state, calculate_account_state, false).into())
        .unwrap();
    let mut leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let mut leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    for leg in [&mut leg_a, &mut leg_b] {
        if let InstrumentAny::FuturesContract(instrument) = leg {
            instrument.currency = Currency::EUR();
        }
    }

    let mut spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    if let InstrumentAny::FuturesSpread(instrument) = &mut spread {
        instrument.currency = Currency::EUR();
    }

    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    for price_type in [PriceType::Bid, PriceType::Ask] {
        assert_eq!(
            cache.borrow().get_xrate(
                spread.id().venue,
                Currency::EUR(),
                Currency::USD(),
                price_type
            ),
            None
        );
    }

    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Cash),
        None,
    );

    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(3))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let cache = cache.borrow();
    let order = cache.order(&order.client_order_id()).unwrap();
    let account = cache.try_account_ref(&account_id).unwrap();
    let fills = saved_fills(&events);

    if calculate_account_state {
        assert_eq!(fills, Vec::new());
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.filled_qty(), Quantity::from(0));
    } else {
        assert_eq!(fills.len(), 3);

        for (index, (instrument, side, price)) in [
            (&leg_a, OrderSide::Sell, "5200.25"),
            (&leg_b, OrderSide::Buy, "18130.75"),
            (&spread, OrderSide::Buy, "12930.50"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(fills[index].instrument_id, instrument.id());
            assert_eq!(fills[index].order_side, side);
            assert_eq!(fills[index].order_type, OrderType::Market);
            assert_eq!(fills[index].last_qty, Quantity::from(3));
            assert_eq!(fills[index].last_px, Price::from(price));
            assert_eq!(fills[index].currency, Currency::EUR());
            assert_eq!(fills[index].commission, Some(Money::from("0 EUR")));
            assert_eq!(fills[index].account_id, account_id);
            assert_eq!(fills[index].liquidity_side, LiquiditySide::Taker);
        }

        assert_eq!(
            fills[0].client_order_id,
            ClientOrderId::from(format!("{}-LEG-ESM4", order.client_order_id()))
        );
        assert_eq!(
            fills[1].client_order_id,
            ClientOrderId::from(format!("{}-LEG-NQM4", order.client_order_id()))
        );
        assert_eq!(fills[2].client_order_id, order.client_order_id());
        assert_eq!(
            fills[0].venue_order_id,
            VenueOrderId::from("XCME-1-1-LEG-0")
        );
        assert_eq!(
            fills[1].venue_order_id,
            VenueOrderId::from("XCME-1-1-LEG-1")
        );
        assert_eq!(fills[2].venue_order_id, VenueOrderId::from("XCME-1-1"));
        assert_eq!(
            fills
                .iter()
                .map(|fill| fill.trade_id)
                .collect::<AHashSet<_>>()
                .len(),
            3
        );
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from(3));
    }

    assert_eq!(account.calculated_account_state(), calculate_account_state);
    assert_eq!(account.base_currency(), Some(Currency::USD()));
    assert_eq!(
        account.balance_total(Some(Currency::USD())),
        Some(Money::from("2500 USD"))
    );
    assert_eq!(
        account.balance_free(Some(Currency::USD())),
        Some(Money::from("2500 USD"))
    );
    assert_eq!(
        account.balance_locked(Some(Currency::USD())),
        Some(Money::from("0 USD"))
    );
    assert_eq!(account.balance_total(Some(Currency::EUR())), None);
    assert_eq!(cache.positions_total_count(None, None, None, None, None), 0);
}

#[rstest]
#[case::gtc(TimeInForce::Gtc)]
#[case::ioc(TimeInForce::Ioc)]
#[case::fok(TimeInForce::Fok)]
fn test_market_to_limit_spread_reports_price_before_slippage(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine(
        spread.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.set_fill_model(
        FillModelAny::Default(DefaultFillModel::new(1.0, 1.0, Some(0)).unwrap()).into(),
    );
    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let effects: Vec<_> = events
        .get_messages()
        .into_iter()
        .filter(|event| matches!(event, OrderEventAny::Updated(_) | OrderEventAny::Filled(_)))
        .collect();
    let fills = saved_fills(&events);
    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(effects.len(), 4);

    let OrderEventAny::Updated(update) = &effects[0] else {
        panic!("Expected initial price update")
    };

    assert_eq!(update.price, Some(Price::from("12930.50")));
    assert_eq!(update.quantity, Quantity::from(1));
    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, price)) in [
        (&leg_a, OrderSide::Sell, "5200.25"),
        (&leg_b, OrderSide::Buy, "18131.00"),
        (&spread, OrderSide::Buy, "12930.75"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_qty, Quantity::from(1));
        assert_eq!(fills[index].last_px, Price::from(price));
    }

    assert_eq!(order.price(), Some(Price::from("12930.50")));
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from(1));
}

#[rstest]
#[case::spread_gtc(true, TimeInForce::Gtc)]
#[case::spread_ioc(true, TimeInForce::Ioc)]
#[case::spread_fok(true, TimeInForce::Fok)]
#[case::ordinary_gtc(false, TimeInForce::Gtc)]
fn test_market_to_limit_slippage_error_preserves_conversion_boundary(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] spread_order: bool,
    #[case] time_in_force: TimeInForce,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    let mut instrument = if spread_order { spread } else { leg_a.clone() };

    if let InstrumentAny::FuturesContract(contract) = &mut instrument {
        contract.activation_ns = UnixNanos::default();
        contract.expiration_ns = UnixNanos::from(2_000_000_000_000_000_000u64);
    }

    for definition in [&leg_a, &leg_b, &instrument] {
        cache
            .borrow_mut()
            .add_instrument(definition.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");
    let mut engine = get_order_matching_engine(
        instrument.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    engine.set_fill_model(FillModelHandle::new(FailingSlippageFillModel {
        book: None,
        checks_before_failure: 0,
    }));

    engine.process_quote_tick(&spread_quote(instrument.id(), "12930.25", "12930.50"));
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let updates: Vec<_> = events
        .get_messages()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Updated(update) => Some(update),
            _ => None,
        })
        .collect();

    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(saved_fills(&events), Vec::new());
    assert_eq!(order.filled_qty(), Quantity::from(0));

    if spread_order {
        assert_eq!(updates, Vec::new());
        assert_eq!(order.price(), None);
        assert_eq!(order.status(), OrderStatus::Canceled);
    } else {
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].price, Some(Price::from("12930.50")));
        assert_eq!(updates[0].quantity, Quantity::from(1));
        assert_eq!(order.price(), Some(Price::from("12930.50")));
        assert_eq!(order.status(), OrderStatus::Accepted);
    }
}

#[rstest]
#[case::gtc_slippage(TimeInForce::Gtc, false)]
#[case::fok_slippage(TimeInForce::Fok, false)]
#[case::gtc_preparation(TimeInForce::Gtc, true)]
#[case::fok_preparation(TimeInForce::Fok, true)]
#[case::ioc_preparation(TimeInForce::Ioc, true)]
fn test_market_to_limit_error_after_first_same_price_fill(
    #[from(spread_account_id)] account_id: AccountId,
    #[case] time_in_force: TimeInForce,
    #[case] preparation_fails: bool,
) {
    let cache = spread_cache(account_id);
    let events = order_event_handler_with_cache(cache.clone());
    let leg_a = futures_contract_with_id("ESM4.XCME", "ESM4");
    let leg_b = futures_contract_with_id("NQM4.XCME", "NQM4");
    let spread = futures_spread_with_generic_id("((1))ESM4___(1)NQM4.XCME", "SPREAD");
    for instrument in [&leg_a, &leg_b, &spread] {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    cache_leg_quote(&cache, leg_a.id(), "5200.25", "5200.50");
    cache_leg_quote(&cache, leg_b.id(), "18130.50", "18130.75");

    let fee_model = if preparation_fails {
        FeeModelHandle::new(FailingFeeModel {
            calls: Cell::new(0),
        })
    } else {
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into()
    };

    let matching = OrderMatchingEngine::new(
        spread.clone(),
        1,
        FillModelHandle::default(),
        fee_model,
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );

    let mut engine = CachingEngine {
        engine: matching,
        cache: cache.clone(),
    };

    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));
    let mut book = OrderBook::new(spread.id(), BookType::L3_MBO);
    for id in [1, 2] {
        book.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from("12930.50"),
                Quantity::from(1),
                id,
            ),
            0,
            id,
            UnixNanos::default(),
        );
    }

    engine.set_fill_model(FillModelHandle::new(FailingSlippageFillModel {
        book: Some(book),
        checks_before_failure: if preparation_fails { 2 } else { 1 },
    }));

    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(spread.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(2))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);

    let effects: Vec<_> = events
        .get_messages()
        .into_iter()
        .filter(|event| matches!(event, OrderEventAny::Updated(_) | OrderEventAny::Filled(_)))
        .collect();
    let initial_fills = saved_fills(&events);
    let initial_order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    if time_in_force == TimeInForce::Fok {
        assert_eq!(effects, Vec::new());
        assert_eq!(initial_order.status(), OrderStatus::Canceled);
        assert_eq!(initial_order.filled_qty(), Quantity::from(0));
        assert_eq!(initial_order.leaves_qty(), Quantity::from(2));
        assert_eq!(initial_order.price(), None);
        return;
    }

    assert_eq!(effects.len(), 4);

    let OrderEventAny::Updated(update) = &effects[0] else {
        panic!("Expected initial price update")
    };

    assert_eq!(update.price, Some(Price::from("12930.50")));
    assert_eq!(update.quantity, Quantity::from(2));
    assert_eq!(initial_fills.len(), 3);

    for fill in &initial_fills {
        assert_eq!(fill.last_qty, Quantity::from(1));
    }

    assert_eq!(initial_fills[2].last_px, Price::from("12930.50"));

    let expected_status = if time_in_force == TimeInForce::Ioc {
        OrderStatus::Canceled
    } else {
        OrderStatus::Accepted
    };

    assert_eq!(initial_order.status(), expected_status);
    assert_eq!(initial_order.filled_qty(), Quantity::from(1));
    assert_eq!(initial_order.leaves_qty(), Quantity::from(1));
    assert_eq!(initial_order.price(), Some(Price::from("12930.50")));

    if time_in_force == TimeInForce::Ioc {
        return;
    }

    engine.set_fill_model(FillModelHandle::default());
    events.clear();
    engine.process_quote_tick(&spread_quote(spread.id(), "12930.25", "12930.50"));

    let fills = saved_fills(&events);
    let order = cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(fills.len(), 3);

    for (index, (instrument, side, price)) in [
        (&leg_a, OrderSide::Sell, "5200.25"),
        (&leg_b, OrderSide::Buy, "18130.75"),
        (&spread, OrderSide::Buy, "12930.50"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fills[index].instrument_id, instrument.id());
        assert_eq!(fills[index].order_side, side);
        assert_eq!(fills[index].last_qty, Quantity::from(1));
        assert_eq!(fills[index].last_px, Price::from(price));
    }

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from(2));
    assert_eq!(order.leaves_qty(), Quantity::from(0));
    assert_eq!(order.price(), Some(Price::from("12930.50")));
}

struct FailingFeeModel {
    calls: Cell<usize>,
}

impl FeeModel for FailingFeeModel {
    fn get_commission(
        &self,
        _order: &OrderAny,
        _fill_quantity: Quantity,
        _fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        let calls = self.calls.get();
        self.calls.set(calls + 1);
        if calls == 2 {
            anyhow::bail!("Commission unavailable");
        }

        Ok(Money::zero(instrument.quote_currency()))
    }
}

struct FailingSlippageFillModel {
    book: Option<OrderBook>,
    checks_before_failure: usize,
}

impl FillModel for FailingSlippageFillModel {
    fn is_limit_filled(&mut self) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn is_slipped(&mut self) -> anyhow::Result<bool> {
        if self.checks_before_failure == 0 {
            anyhow::bail!("Slippage unavailable");
        }

        self.checks_before_failure -= 1;
        Ok(false)
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        _instrument: &InstrumentAny,
        _order: &OrderAny,
        _best_bid: Option<Price>,
        _best_ask: Option<Price>,
    ) -> anyhow::Result<Option<OrderBook>> {
        Ok(self.book.clone())
    }
}

#[fixture]
fn spread_account_id() -> AccountId {
    AccountId::from("ACCOUNT-001")
}

fn saved_fills(events: &TypedIntoMessageSavingHandler<OrderEventAny>) -> Vec<OrderFilled> {
    events
        .get_messages()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect()
}

fn spread_cache(account_id: AccountId) -> Rc<RefCell<Cache>> {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut state = margin_account_state();
    state.account_id = account_id;
    cache
        .borrow_mut()
        .add_account(MarginAccount::new(state, false).into())
        .unwrap();
    cache
}

fn cache_leg_quote(cache: &Rc<RefCell<Cache>>, instrument_id: InstrumentId, bid: &str, ask: &str) {
    cache
        .borrow_mut()
        .add_quote(spread_quote(instrument_id, bid, ask))
        .unwrap();
}

fn spread_quote(instrument_id: InstrumentId, bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(10),
        Quantity::from(10),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn futures_contract_with_id(instrument_id: &str, symbol: &str) -> InstrumentAny {
    let mut contract = futures_contract_es(None, None);
    contract.id = InstrumentId::from(instrument_id);
    contract.raw_symbol = Symbol::from(symbol);
    contract.price_increment = Price::from("0.25");
    InstrumentAny::FuturesContract(contract)
}

fn futures_spread_with_generic_id(instrument_id: &str, symbol: &str) -> InstrumentAny {
    InstrumentAny::FuturesSpread(
        FuturesSpread::build_checked(
            InstrumentId::from(instrument_id),
            Symbol::from(symbol),
            AssetClass::Index,
            Some(Ustr::from("XCME")),
            Ustr::from("ES_NQ"),
            Ustr::from("SPREAD"),
            UnixNanos::default(),
            UnixNanos::from(2_000_000_000_000_000_000u64),
            Currency::USD(),
            2,
            Price::from("0.25"),
            Quantity::from(1),
            Quantity::from(1),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
        )
        .unwrap(),
    )
}
