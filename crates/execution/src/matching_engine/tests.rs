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
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use nautilus_common::{cache::Cache, clock::VirtualClock, messages::execution::ModifyOrder};
use nautilus_core::{UUID4, UnixNanos, correctness::CorrectnessError};
#[cfg(feature = "high-precision")]
use nautilus_model::orderbook::BookLevel;
use nautilus_model::{
    data::{
        Bar, BarType, DEPTH10_LEN, OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick,
        TradeTick,
        option_chain::OptionGreeks,
        order::{BookOrder, OrderId},
    },
    enums::{
        AccountType, AggressorSide, BookAction, BookType, ContingencyType, LiquiditySide,
        MarketStatus, MarketStatusAction, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
        RecordFlag, TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{OrderEventAny, OrderEventType, OrderReleased},
    identifiers::{AccountId, ClientOrderId, TradeId, VenueOrderId},
    instruments::{
        Instrument, InstrumentAny,
        stubs::{
            crypto_option_btc_deribit, crypto_perpetual_ethusdt, currency_pair_ethusdt,
            futures_contract_es, tokenized_asset_aaplx,
        },
    },
    orderbook::OrderBook,
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    types::{Currency, Money, Price, Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};
use proptest::prelude::*;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{
    BarTickSizes, OrderFilled, OrderMatchingEngine, Position, PositionId, PostMatchOrderAction,
    order_precision_valid, post_match_order_action,
};
use crate::{
    matching_engine::config::OrderMatchingEngineConfig,
    models::{
        fee::{FeeModel, FeeModelAny, FeeModelHandle, MakerTakerFeeModel},
        fill::{DefaultFillModel, FillModel, FillModelAny, FillModelHandle},
    },
};

// Caches the order before processing, as the execution engine does
fn process_cached_order(
    engine: &mut OrderMatchingEngine,
    order: &mut OrderAny,
    account_id: AccountId,
) {
    if !engine.cache.borrow().order_exists(&order.client_order_id()) {
        engine
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    engine.process_order(order, account_id);
}

fn assert_valid_bar_tick_sizes(volume: Quantity, size_increment: Quantity) {
    let sizes = BarTickSizes::from_volume(volume, size_increment);
    let total_raw = sizes.open.raw() + sizes.high.raw() + sizes.low.raw() + sizes.close.raw();
    assert!(total_raw <= volume.raw());

    for quantity in [sizes.open, sizes.high, sizes.low, sizes.close] {
        assert_eq!(quantity.precision, volume.precision);
        assert!(
            OrderMatchingEngine::quantity_matches_precision(quantity, volume.precision),
            "bar tick quantity {quantity} not aligned to precision {}",
            volume.precision,
        );
        assert!(
            size_increment.is_zero() || quantity.raw().is_multiple_of(size_increment.raw()),
            "bar tick quantity {quantity} not aligned to increment {size_increment}",
        );
    }

    if size_increment.is_positive() {
        assert!(
            volume.raw() - total_raw < size_increment.raw(),
            "bar tick split left {} raw units from volume {volume} and increment {size_increment}",
            volume.raw() - total_raw,
        );
    }
}

#[rstest]
#[case::lower(0, FIXED_PRECISION, true)]
#[case::equal(FIXED_PRECISION, FIXED_PRECISION, true)]
#[case::excess(3, 2, false)]
#[case::native_equal(18, 18, true)]
#[case::native_lower(17, 18, false)]
#[case::native_shared_lower(FIXED_PRECISION, 18, false)]
fn test_order_precision_valid(#[case] actual: u8, #[case] expected: u8, #[case] accepted: bool) {
    assert_eq!(order_precision_valid(actual, expected), accepted);
}

#[rstest]
#[case("100.009", "100.011", "100.000", true)]
#[case("100.009", "100.020", "100.008", false)]
#[case("100.010", "100.020", "100.000", false)]
fn test_bar_high_first_preserves_stored_distances(
    #[case] open: &str,
    #[case] high: &str,
    #[case] low: &str,
    #[case] expected: bool,
) {
    let mut engine = OrderMatchingEngine::new(
        InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::new(RefCell::new(Cache::default())),
        Default::default(),
    );
    engine.config.bar_adaptive_high_low_ordering = true;
    let mut prices = [Price::from(open), Price::from(high), Price::from(low)];
    for price in &mut prices {
        price.precision = 2;
    }

    let bar = Bar::new(
        BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"),
        prices[0],
        prices[1],
        prices[2],
        prices[0],
        Quantity::from("1.000"),
        1.into(),
        1.into(),
    );
    assert_eq!(engine.bar_high_first(&bar), expected);
}

#[cfg(feature = "high-precision")]
#[rstest]
fn test_consume_trade_level_preserves_native_raw_units() {
    let precision = if Quantity::from_raw_checked(0, 18).is_ok() {
        18
    } else {
        FIXED_PRECISION
    };

    let size = Quantity::from_raw(2_000_000_000_000_000_000, precision);
    let level = BookLevel::from_order(BookOrder::new(
        OrderSide::Sell,
        Price::from("1.00"),
        size,
        1,
    ));
    let mut consumption = indexmap::IndexMap::default();
    let mut remaining = size.raw();
    OrderMatchingEngine::consume_trade_level(&mut consumption, &mut remaining, &level);
    assert_eq!(remaining, 0);
    assert_eq!(
        consumption[&level.price.value.raw()],
        (size.raw(), size.raw())
    );
}

#[rstest]
fn test_post_match_order_action_does_not_clone_no_maintenance_order() {
    let order = post_match_limit_order();
    let clone_count = Cell::new(0);
    let action = post_match_order_action(&order, true, UnixNanos::from(1_u64), |order| {
        clone_count.set(clone_count.get() + 1);
        order.clone()
    });

    assert!(matches!(action, PostMatchOrderAction::NoMaintenance));
    assert_eq!(clone_count.get(), 0);
}

#[rstest]
#[case::spread(
    InstrumentAny::FuturesSpread(nautilus_model::instruments::stubs::futures_spread_es()),
    0
)]
#[case::outright(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()), 1)]
fn test_pending_fills_exclude_instruments_without_positions(
    #[case] instrument: InstrumentAny,
    #[case] expected_pending: usize,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (order, fill) = pending_position_fill(
        &instrument,
        PositionId::from("POSITION-001"),
        "OPEN",
        OrderSide::Buy,
        "1",
    );
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();
    engine.record_pending_fill(&fill);
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(fill))
        .unwrap();
    engine.purge_applied_fills();
    assert_eq!(engine.pending_fills.len(), expected_pending);
}

#[rstest]
fn test_pending_fills_wait_for_position_acknowledgement(
    #[values(OmsType::Netting, OmsType::Hedging)] oms_type: OmsType,
    #[values(OrderSide::Buy, OrderSide::Sell)] closing_side: OrderSide,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        oms_type,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let position_id = PositionId::from("POSITION-001");

    let opening_side = if closing_side == OrderSide::Buy {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };

    let (opening, opening_fill) =
        pending_position_fill(&instrument, position_id, "OPEN", opening_side, "0.500");
    let (closing, first_fill) = pending_position_fill(
        &instrument,
        position_id,
        "CLOSE-FIRST",
        closing_side,
        "0.400",
    );
    let (_, second_fill) = pending_position_fill(
        &instrument,
        position_id,
        "CLOSE-SECOND",
        closing_side,
        "0.100",
    );
    let (unrelated, unrelated_fill) = pending_position_fill(
        &instrument,
        PositionId::from("POSITION-002"),
        "UNRELATED",
        closing_side,
        "0.200",
    );
    let position = Position::new(&instrument, opening_fill);
    cache
        .borrow_mut()
        .add_order(opening, None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, oms_type)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    engine.record_pending_fill(&first_fill);
    cache
        .borrow_mut()
        .add_order(unrelated, None, None, false)
        .unwrap();
    engine.record_pending_fill(&unrelated_fill);
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("0.100")
    );

    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(first_fill.clone()))
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("0.100")
    );
    let position = cache
        .borrow_mut()
        .update_position_from_fill(position_id, &first_fill)
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("0.100")
    );
    assert!(!engine.pending_fills.contains_key(&first_fill.trade_id));

    engine.record_pending_fill(&second_fill);
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("0.000")
    );
    engine.reset();
    assert!(engine.pending_fills.is_empty());
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("0.100")
    );
}

#[rstest]
#[case::currency_pair_base_fee(InstrumentAny::CurrencyPair(currency_pair_ethusdt()), true, 0.89)]
#[case::currency_pair_quote_fee(InstrumentAny::CurrencyPair(currency_pair_ethusdt()), false, 0.90)]
#[case::tokenized_asset_base_fee(
    InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()),
    true,
    0.89
)]
#[case::tokenized_asset_quote_fee(
    InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()),
    false,
    0.90
)]
fn test_pending_spot_fills_include_base_currency_commission(
    #[case] instrument: InstrumentAny,
    #[case] base_fee: bool,
    #[case] expected: f64,
) {
    let qty = |value: f64| Quantity::new(value, instrument.size_precision());
    let commission_currency = if base_fee {
        instrument.base_currency().unwrap()
    } else {
        instrument.quote_currency()
    };

    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Cash,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let position_id = PositionId::from("POSITION-001");
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "OPEN",
        OrderSide::Buy,
        &qty(0.5).to_string(),
    );
    let (_, mut increase_fill) = pending_position_fill(
        &instrument,
        position_id,
        "INCREASE",
        OrderSide::Buy,
        &qty(0.4).to_string(),
    );
    let (closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "CLOSE",
        OrderSide::Sell,
        &qty(1.0).to_string(),
    );
    increase_fill.commission = Some(Money::new(0.01, commission_currency));
    let position = Position::new(&instrument, opening_fill);
    cache
        .borrow_mut()
        .add_order(opening, None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    engine.record_pending_fill(&increase_fill);
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        qty(expected)
    );
    let position = cache
        .borrow_mut()
        .update_position_from_fill(position_id, &increase_fill)
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        qty(expected)
    );
    assert!(engine.pending_fills.is_empty());
}

#[rstest]
fn test_pending_fills_survive_position_flip_and_archive_acknowledgement() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let position_id = PositionId::from("POSITION-001");
    let (opening, opening_fill) =
        pending_position_fill(&instrument, position_id, "OPEN", OrderSide::Buy, "10.000");
    let (flipping, flip_fill) =
        pending_position_fill(&instrument, position_id, "FLIP", OrderSide::Sell, "15.000");
    let (closing, close_fill) =
        pending_position_fill(&instrument, position_id, "CLOSE", OrderSide::Buy, "4.000");

    for order in [opening, flipping, closing.clone()] {
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
    }

    let mut position = Position::new(&instrument, opening_fill);
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    engine.record_pending_fill(&flip_fill);
    engine.record_pending_fill(&close_fill);
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("1.000")
    );

    let (closing_flip, opening_flip) = flip_fill
        .split_for_position_flip(Quantity::from("10.000"), Some(position_id), UUID4::new())
        .unwrap();
    position.apply(&closing_flip);
    cache.borrow_mut().snapshot_position(&position).unwrap();
    let position = Position::new(&instrument, opening_flip);
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("1.000")
    );
    assert!(!engine.pending_fills.contains_key(&flip_fill.trade_id));
    assert!(engine.pending_fills.contains_key(&close_fill.trade_id));

    let position = cache
        .borrow_mut()
        .update_position_from_fill(position_id, &close_fill)
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("1.000")
    );
    assert!(engine.pending_fills.is_empty());

    let (_, flatten_fill) =
        pending_position_fill(&instrument, position_id, "FLATTEN", OrderSide::Buy, "1.000");
    let (_, reopen_fill) =
        pending_position_fill(&instrument, position_id, "REOPEN", OrderSide::Sell, "3.000");
    engine.record_pending_fill(&flatten_fill);
    engine.record_pending_fill(&reopen_fill);
    cache
        .borrow_mut()
        .update_position_from_fill(position_id, &flatten_fill)
        .unwrap();
    let closed = cache.borrow().position(&position_id).unwrap().clone();
    cache.borrow_mut().snapshot_position(&closed).unwrap();
    let position = Position::new(&instrument, reopen_fill);
    cache
        .borrow_mut()
        .add_position_without_order(&position, OmsType::Netting)
        .unwrap();
    assert_eq!(
        engine
            .position_quantity_remaining(&closing, Some(&position))
            .unwrap(),
        Quantity::from("3.000")
    );
    assert!(engine.pending_fills.is_empty());
}

#[rstest]
#[case(("0.000", "0.000"), (None, None), "open", (["6.000", "4.000"], [Some("6.000"), Some("4.000")]), false)]
#[case(("2.000", "3.000"), (None, None), "open", (["8.000", "6.000"], [Some("9.000"), Some("7.000")]), false)]
#[case(("2.000", "3.000"), (Some("7.000"), Some("6.000")), "open", (["7.000", "6.000"], [Some("6.000"), None]), false)]
#[case(("2.000", "3.000"), (Some("2.000"), None), "open", (["2.000", "2.000"], [None, None]), true)]
#[case(("2.000", "3.000"), (None, Some("3.000")), "open", (["8.000", "6.000"], [Some("3.000"), None]), true)]
#[case(("0.000", "0.000"), (None, None), "closed", (["6.000", "4.000"], [None, None]), false)]
#[case(("0.000", "0.000"), (None, None), "local", (["6.000", "4.000"], [None, None]), false)]
#[case(("0.000", "0.000"), (None, None), "cancellation_unacknowledged", (["6.000", "4.000"], [None, None]), false)]
fn test_position_sync_resizes_mixed_ouo_sibling(
    #[case] filled: (&str, &str),
    #[case] parents: (Option<&str>, Option<&str>),
    #[case] sibling_state: &str,
    #[case] expected: ([&str; 2], [Option<&str>; 2]),
    #[case] first_cancel: bool,
    #[values(0, 1, 2)] delivery: usize,
    #[values(false, true)] support_contingent_orders: bool,
) {
    let (source_filled, sibling_filled) = filled;
    let (source_parent, sibling_parent) = parents;
    let (source_quantities, sibling_updates) = expected;
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("MIXED-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig {
            support_contingent_orders,
            ..Default::default()
        },
    );

    let opening_quantity =
        Quantity::from("10.000") + Quantity::from(source_filled) + Quantity::from(sibling_filled);
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "MIXED-OPEN",
        OrderSide::Buy,
        &opening_quantity.to_string(),
    );
    let mut position = Position::new(&instrument, opening_fill.clone());
    engine
        .account_ids
        .insert(position.trader_id, position.account_id);
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_cache.borrow_mut().update_order(&event).unwrap();
    }));

    for (id, sibling, reduce_only, filled, parent_quantity) in [
        ("MIXED-A", "MIXED-B", true, source_filled, source_parent),
        ("MIXED-B", "MIXED-A", false, sibling_filled, sibling_parent),
    ] {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);

        if let Some(quantity) = parent_quantity {
            let parent_id = format!("{id}-PARENT");
            let (parent, mut fill) = pending_position_fill(
                &instrument,
                position_id,
                &parent_id,
                OrderSide::Buy,
                quantity,
            );
            fill.venue_order_id = VenueOrderId::from(parent_id.as_str());
            cache
                .borrow_mut()
                .add_order(parent, Some(position_id), None, false)
                .unwrap();
            cache
                .borrow_mut()
                .update_order(&OrderEventAny::Filled(fill))
                .unwrap();
            builder.parent_order_id(ClientOrderId::from(parent_id));
        }

        let mut order = builder
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.000"))
            .price(Price::from("2000.00"))
            .reduce_only(reduce_only)
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![ClientOrderId::from(sibling)])
            .submit(sibling_state != "local" || reduce_only)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();

        if sibling_state != "local" || reduce_only {
            engine.accept_order(&mut order);
        }

        if Quantity::from(filled).non_zero() {
            let (_, mut fill) =
                pending_position_fill(&instrument, position_id, id, OrderSide::Sell, filled);
            fill.venue_order_id = order.venue_order_id().unwrap();
            fill.order_type = OrderType::Limit;
            position.apply(&fill);
            cache
                .borrow_mut()
                .update_order(&OrderEventAny::Filled(fill))
                .unwrap();
        }

        if !reduce_only && sibling_state == "closed" {
            engine.cancel_order(&order, Some(false));
        }
    }

    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if delivery == 0 {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    if sibling_state == "cancellation_unacknowledged" {
        let sibling = engine
            .order_snapshot(ClientOrderId::from("MIXED-B"))
            .unwrap();
        engine.cancel_order(&sibling, Some(false));
    }

    let (closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "MIXED-CLOSE",
        OrderSide::Sell,
        "10.000",
    );
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    let mut acknowledged = 0;
    let mut source_quantity = Quantity::from("10.000");
    let mut sibling_quantity = Quantity::from("10.000");
    let mut source_canceled = false;
    let mut sibling_canceled = matches!(sibling_state, "closed" | "cancellation_unacknowledged");

    for (step, (quantity, remaining)) in
        [("4.000", "6.000"), ("2.000", "4.000"), ("4.000", "0.000")]
            .into_iter()
            .enumerate()
    {
        let start = events.borrow().len();
        engine
            .apply_fills(
                &closing,
                &[(Price::from("1000.00"), Quantity::from(quantity))],
                LiquiditySide::Taker,
                Some(position_id),
                Some(&position),
                None,
                false,
            )
            .unwrap();
        let mut expected = vec![("fill", "MIXED-CLOSE", Quantity::from(quantity))];

        if !source_canceled {
            if step < 2 {
                let target = if support_contingent_orders {
                    Quantity::from(source_quantities[step])
                } else {
                    Quantity::from(source_filled) + Quantity::from(remaining)
                };

                if target != source_quantity {
                    expected.push(("update", "MIXED-A", target));
                    source_quantity = target;

                    if support_contingent_orders && source_parent == Some(source_filled) {
                        expected.push(("cancel", "MIXED-A", Quantity::zero(3)));
                        source_canceled = true;

                        if !sibling_canceled && sibling_state != "local" {
                            expected.push(("cancel", "MIXED-B", Quantity::zero(3)));
                            sibling_canceled = true;
                        }
                    } else if support_contingent_orders {
                        if let Some(target) = sibling_updates[step] {
                            sibling_quantity = Quantity::from(target);
                            expected.push(("update", "MIXED-B", sibling_quantity));
                        }

                        if step == 0 && first_cancel {
                            expected.push(("cancel", "MIXED-B", Quantity::zero(3)));
                            sibling_canceled = true;
                        }
                    }
                }
            } else {
                expected.push(("cancel", "MIXED-A", Quantity::zero(3)));
                source_canceled = true;

                if support_contingent_orders && !sibling_canceled && sibling_state != "local" {
                    expected.push(("cancel", "MIXED-B", Quantity::zero(3)));
                    sibling_canceled = true;
                }
            }
        }

        let recorded = events.borrow();

        let actual: Vec<_> = recorded[start..]
            .iter()
            .map(|event| match event {
                OrderEventAny::Filled(fill) => {
                    assert_eq!(fill.last_px, Price::from("1000.00"));
                    ("fill", fill.client_order_id.as_str(), fill.last_qty)
                }
                OrderEventAny::Updated(update) => {
                    assert_eq!(update.price, Some(Price::from("2000.00")));
                    assert_eq!(update.trigger_price, None);
                    ("update", update.client_order_id.as_str(), update.quantity)
                }
                OrderEventAny::Canceled(cancel) => {
                    ("cancel", cancel.client_order_id.as_str(), Quantity::zero(3))
                }
                other => panic!("Unexpected event {other:?}"),
            })
            .collect();

        assert_eq!(actual, expected);
        drop(recorded);

        if delivery == 2 {
            let end = events.borrow().len() - 1;
            for event in &events.borrow()[acknowledged..end] {
                cache.borrow_mut().update_order(event).unwrap();
                if let OrderEventAny::Filled(fill) = event {
                    cache
                        .borrow_mut()
                        .update_position_from_fill(position_id, fill)
                        .unwrap();
                }
            }

            acknowledged = end;
        }

        let before = events.borrow().len();
        let ids = engine.reduce_only_order_ids(position_id);
        engine
            .sync_reduce_only_orders(&closing, Some(&position), &ids)
            .unwrap();
        assert_eq!(events.borrow().len(), before);
    }

    if delivery != 0 {
        for event in &events.borrow()[acknowledged..] {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    for (id, filled, quantity, canceled) in [
        ("MIXED-A", source_filled, source_quantity, source_canceled),
        (
            "MIXED-B",
            sibling_filled,
            sibling_quantity,
            sibling_canceled,
        ),
    ] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();
        assert_eq!(order.quantity(), quantity);
        assert_eq!(order.filled_qty(), Quantity::from(filled));
        assert_eq!(order.leaves_qty(), quantity - Quantity::from(filled));
        assert_eq!(
            order.status(),
            if canceled {
                OrderStatus::Canceled
            } else if sibling_state == "local" {
                OrderStatus::Initialized
            } else if Quantity::from(filled).is_zero() {
                OrderStatus::Accepted
            } else {
                OrderStatus::PartiallyFilled
            }
        );
        assert_eq!(
            engine.order_exists(order.client_order_id()),
            !canceled && sibling_state != "local"
        );
    }

    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("0.000")
    );
    assert_eq!(
        cache.position(&position_id).unwrap().side,
        PositionSide::Flat
    );
}

#[rstest]
#[case("5.000", "5.000", false)]
#[case("10.000", "0.000", true)]
fn test_position_sync_handles_unacknowledged_sibling_acceptance(
    #[case] closing_quantity: &str,
    #[case] remaining_quantity: &str,
    #[case] canceled: bool,
    #[values(false, true)] deferred: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("REENTRANT-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-OPEN",
        OrderSide::Buy,
        "10.000",
    );
    let position = Position::new(&instrument, opening_fill.clone());
    engine
        .account_ids
        .insert(position.trader_id, position.account_id);
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    let sibling_id = ClientOrderId::from("ACCEPT-B");
    engine.set_event_handler(Rc::new(move |event| {
        let id = match &event {
            OrderEventAny::Accepted(event) => event.client_order_id,
            OrderEventAny::Filled(event) => event.client_order_id,
            OrderEventAny::Canceled(event) => event.client_order_id,
            OrderEventAny::Updated(event) => event.client_order_id,
            other => panic!("Unexpected event {other:?}"),
        };

        let applied =
            id != sibling_id && (!deferred || matches!(event, OrderEventAny::Accepted(_)));

        if applied {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push((event, applied));
    }));

    for (id, sibling, reduce_only) in [
        ("ACCEPT-A", "ACCEPT-B", true),
        ("ACCEPT-B", "ACCEPT-A", false),
    ] {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.000"))
            .price(Price::from("2000.00"))
            .reduce_only(reduce_only)
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![ClientOrderId::from(sibling)])
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();
        engine.accept_order(&mut order);
    }

    assert_eq!(
        cache.borrow().order(&sibling_id).unwrap().status(),
        OrderStatus::Submitted
    );
    assert!(engine.order_exists(sibling_id));
    let (closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "ACCEPT-CLOSE",
        OrderSide::Sell,
        closing_quantity,
    );
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    engine
        .apply_fills(
            &closing,
            &[(Price::from("1000.00"), Quantity::from(closing_quantity))],
            LiquiditySide::Taker,
            Some(position_id),
            Some(&position),
            None,
            false,
        )
        .unwrap();
    let ids = engine.reduce_only_order_ids(position_id);
    engine
        .sync_reduce_only_orders(&closing, Some(&position), &ids)
        .unwrap();
    let events = events.borrow();

    let actual: Vec<_> = events
        .iter()
        .map(|(event, _)| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.client_order_id.as_str()),
            OrderEventAny::Filled(fill) => {
                assert_eq!(fill.last_qty, Quantity::from(closing_quantity));
                assert_eq!(fill.last_px, Price::from("1000.00"));
                ("filled", fill.client_order_id.as_str())
            }
            OrderEventAny::Updated(event) => {
                assert_eq!(event.quantity, Quantity::from("5.000"));
                assert_eq!(event.price, Some(Price::from("2000.00")));
                assert_eq!(event.trigger_price, None);
                ("updated", event.client_order_id.as_str())
            }
            OrderEventAny::Canceled(event) => ("canceled", event.client_order_id.as_str()),
            other => panic!("Unexpected event {other:?}"),
        })
        .collect();

    let mut expected = vec![
        ("accepted", "ACCEPT-A"),
        ("accepted", "ACCEPT-B"),
        ("filled", "ACCEPT-CLOSE"),
    ];

    if canceled {
        expected.extend([("canceled", "ACCEPT-A"), ("canceled", "ACCEPT-B")]);
    } else {
        expected.push(("updated", "ACCEPT-A"));
    }

    assert_eq!(actual, expected);
    assert_eq!(engine.order_exists(sibling_id), !canceled);

    for (event, applied) in events.iter() {
        if !applied {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    for id in ["ACCEPT-A", "ACCEPT-B"] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();

        let quantity = Quantity::from(if !canceled && id == "ACCEPT-A" {
            "5.000"
        } else {
            "10.000"
        });

        assert_eq!(
            order.status(),
            if canceled {
                OrderStatus::Canceled
            } else {
                OrderStatus::Accepted
            }
        );
        assert_eq!(order.quantity(), quantity);
        assert_eq!(order.filled_qty(), Quantity::from("0.000"));
        assert_eq!(order.leaves_qty(), quantity);
    }

    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from(remaining_quantity)
    );
    assert_eq!(
        cache.position(&position_id).unwrap().side,
        if canceled {
            PositionSide::Flat
        } else {
            PositionSide::Long
        }
    );
}

#[rstest]
fn test_position_sync_mixed_ouo_does_not_match_recursively(#[values(0, 1, 2)] delivery: usize) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("REENTRANT-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-OPEN",
        OrderSide::Buy,
        "10.000",
    );
    let position = Position::new(&instrument, opening_fill.clone());
    engine
        .account_ids
        .insert(position.trader_id, position.account_id);
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    for (id, price, size) in [(1, "1000.00", "1.000"), (2, "999.00", "9.000")] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                instrument.id(),
                BookAction::Add,
                BookOrder::new(OrderSide::Buy, Price::from(price), Quantity::from(size), id),
                0,
                id,
                UnixNanos::from(id),
                UnixNanos::from(id),
            ))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if delivery == 0 || matches!(event, OrderEventAny::Accepted(_)) {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    for (id, sibling, reduce_only) in [
        ("REENTRANT-A", "REENTRANT-B", true),
        ("REENTRANT-B", "REENTRANT-A", false),
    ] {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.000"))
            .price(Price::from("999.00"))
            .reduce_only(reduce_only)
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![ClientOrderId::from(sibling)])
            .submit(true);
        let mut order = builder.build();
        order.set_liquidity_side(LiquiditySide::Taker);
        cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();
        engine.accept_order(&mut order);
    }

    events.borrow_mut().clear();

    let (mut closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-CLOSE",
        OrderSide::Sell,
        "4.000",
    );
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    engine.process_order(&mut closing, position.account_id);
    let mut acknowledged = 0;

    if delivery == 2 {
        for event in &events.borrow()[..5] {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        acknowledged = 5;
    }

    assert_eq!(
        engine
            .position_quantity_remaining(
                &closing,
                Some(&cache.borrow().position(&position_id).unwrap())
            )
            .unwrap(),
        Quantity::from("6.000")
    );

    for id in ["REENTRANT-A", "REENTRANT-B"] {
        let order = engine.order_snapshot(ClientOrderId::from(id)).unwrap();
        assert_eq!(order.quantity(), Quantity::from("6.000"));
        assert_eq!(order.filled_qty(), Quantity::from("0.000"));
        assert_eq!(order.leaves_qty(), Quantity::from("6.000"));
    }

    let (mut flattening, _) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-FLAT",
        OrderSide::Sell,
        "6.000",
    );
    cache
        .borrow_mut()
        .add_order(flattening.clone(), Some(position_id), None, false)
        .unwrap();
    engine.process_order(&mut flattening, position.account_id);
    let recorded = events.borrow();

    let actual: Vec<_> = recorded
        .iter()
        .map(|event| match event {
            OrderEventAny::Filled(fill) => (
                "fill",
                fill.client_order_id.as_str(),
                fill.last_qty,
                Some(fill.last_px),
            ),
            OrderEventAny::Updated(update) => {
                assert_eq!(update.trigger_price, None);
                (
                    "update",
                    update.client_order_id.as_str(),
                    update.quantity,
                    update.price,
                )
            }
            OrderEventAny::Canceled(cancel) => (
                "cancel",
                cancel.client_order_id.as_str(),
                Quantity::zero(3),
                None,
            ),
            other => panic!("Unexpected event {other:?}"),
        })
        .collect();

    assert_eq!(
        actual,
        vec![
            (
                "fill",
                "REENTRANT-CLOSE",
                Quantity::from("1.000"),
                Some(Price::from("1000.00"))
            ),
            (
                "update",
                "REENTRANT-A",
                Quantity::from("9.000"),
                Some(Price::from("999.00"))
            ),
            (
                "update",
                "REENTRANT-B",
                Quantity::from("9.000"),
                Some(Price::from("999.00"))
            ),
            (
                "fill",
                "REENTRANT-CLOSE",
                Quantity::from("3.000"),
                Some(Price::from("999.00"))
            ),
            (
                "update",
                "REENTRANT-A",
                Quantity::from("6.000"),
                Some(Price::from("999.00"))
            ),
            (
                "update",
                "REENTRANT-B",
                Quantity::from("6.000"),
                Some(Price::from("999.00"))
            ),
            (
                "fill",
                "REENTRANT-FLAT",
                Quantity::from("1.000"),
                Some(Price::from("1000.00"))
            ),
            (
                "update",
                "REENTRANT-A",
                Quantity::from("5.000"),
                Some(Price::from("999.00"))
            ),
            (
                "update",
                "REENTRANT-B",
                Quantity::from("5.000"),
                Some(Price::from("999.00"))
            ),
            (
                "fill",
                "REENTRANT-FLAT",
                Quantity::from("5.000"),
                Some(Price::from("999.00"))
            ),
            ("cancel", "REENTRANT-A", Quantity::zero(3), None),
            ("cancel", "REENTRANT-B", Quantity::zero(3), None),
        ]
    );

    if delivery != 0 {
        for event in &recorded[acknowledged..] {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("0.000")
    );
    assert_eq!(
        cache.position(&position_id).unwrap().side,
        PositionSide::Flat
    );

    for id in ["REENTRANT-A", "REENTRANT-B"] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.quantity(), Quantity::from("5.000"));
        assert_eq!(order.filled_qty(), Quantity::from("0.000"));
        assert_eq!(order.leaves_qty(), Quantity::from("5.000"));
        assert!(!engine.order_exists(order.client_order_id()));
    }
}

#[rstest]
fn test_position_sync_includes_newly_activated_oto_child(#[values(false, true)] deferred: bool) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("ACTIVATION-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "ACTIVATION-OPEN",
        OrderSide::Buy,
        "10.000",
    );
    let position = Position::new(&instrument, opening_fill.clone());
    engine
        .account_ids
        .insert(position.trader_id, position.account_id);
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let parent_id = ClientOrderId::from("ACTIVATION-PARENT");
    let child_id = ClientOrderId::from("ACTIVATION-CHILD");
    let parent = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(parent_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .submit(true)
        .build();
    let child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("2000.00"))
        .reduce_only(true)
        .parent_order_id(parent_id)
        .submit(true)
        .build();

    for order in [parent.clone(), child] {
        cache
            .borrow_mut()
            .add_order(order, Some(position_id), None, false)
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred || matches!(event, OrderEventAny::Accepted(_)) {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    assert!(!engine.order_exists(child_id));

    engine
        .apply_fills(
            &parent,
            &[(Price::from("1000.00"), Quantity::from("2.000"))],
            LiquiditySide::Taker,
            Some(position_id),
            Some(&position),
            None,
            false,
        )
        .unwrap();

    let events = events.borrow();
    assert_eq!(events.len(), 3);
    assert!(
        matches!(&events[0], OrderEventAny::Filled(fill) if fill.client_order_id == parent_id && fill.last_qty == Quantity::from("2.000"))
    );
    assert!(
        matches!(&events[1], OrderEventAny::Accepted(accepted) if accepted.client_order_id == child_id)
    );

    let OrderEventAny::Updated(update) = &events[2] else {
        panic!("Expected child quantity update")
    };

    assert_eq!(update.client_order_id, child_id);
    assert_eq!(update.quantity, Quantity::from("2.000"));
    assert_eq!(update.price, Some(Price::from("2000.00")));
    assert_eq!(update.trigger_price, None);
    assert!(engine.order_exists(child_id));
    assert_eq!(
        engine.order_snapshot(child_id).unwrap().quantity(),
        Quantity::from("2.000")
    );

    if deferred {
        for event in events.iter() {
            if matches!(event, OrderEventAny::Accepted(_)) {
                continue;
            }

            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("12.000")
    );
    let child = cache.order(&child_id).unwrap();
    assert_eq!(child.status(), OrderStatus::Accepted);
    assert_eq!(child.quantity(), Quantity::from("2.000"));
    assert_eq!(child.filled_qty(), Quantity::from("0.000"));
    assert_eq!(child.leaves_qty(), Quantity::from("2.000"));
}

fn pending_position_fill(
    instrument: &InstrumentAny,
    position_id: PositionId,
    id: &str,
    side: OrderSide,
    quantity: &str,
) -> (OrderAny, OrderFilled) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(Quantity::from(quantity))
        .submit(true)
        .build();

    let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from(id)),
        Some(position_id),
        Some(Price::from("1000.00")),
        None,
        None,
        Some(Money::zero(instrument.quote_currency())),
        None,
        None,
    ) else {
        unreachable!()
    };

    (order, fill)
}

#[rstest]
fn test_pending_modify_updates_acknowledge_individually_and_reset() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("99.00"))
        .submit(true)
        .build();
    let id = order.client_order_id();
    engine.set_event_handler(Rc::new(|_| {}));
    process_cached_order(&mut engine, &mut order, AccountId::from("ACCOUNT-001"));
    let pending = Rc::new(RefCell::new(Vec::new()));
    let events = pending.clone();
    engine.set_event_handler(Rc::new(move |event| events.borrow_mut().push(event)));
    for (quantity, price) in [
        (Some(Quantity::from("2.000")), None),
        (None, Some(Price::from("100.00"))),
    ] {
        engine.process_modify(
            &ModifyOrder::new(
                order.trader_id(),
                None,
                order.strategy_id(),
                order.instrument_id(),
                id,
                None,
                quantity,
                price,
                None,
                UUID4::new(),
                UnixNanos::from(1),
                None,
                None,
            ),
            AccountId::from("ACCOUNT-001"),
        );
    }

    assert_eq!(pending.borrow().len(), 2);
    cache
        .borrow_mut()
        .update_order(&pending.borrow()[0])
        .unwrap();
    let snapshot = engine.order_snapshot(id).unwrap();
    assert_eq!(snapshot.quantity(), Quantity::from("2.000"));
    assert_eq!(snapshot.price(), Some(Price::from("100.00")));
    assert_eq!(engine.orders.pending_updates.borrow()[&id].len(), 1);
    cache
        .borrow_mut()
        .update_order(&pending.borrow()[1])
        .unwrap();
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert!(engine.orders.pending_updates.borrow().is_empty());
    engine.process_modify(
        &ModifyOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            id,
            None,
            Some(Quantity::from("3.000")),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(3),
            None,
            None,
        ),
        AccountId::from("ACCOUNT-001"),
    );
    assert_eq!(
        engine.order_snapshot(id).unwrap().quantity(),
        Quantity::from("3.000")
    );
    engine.reset();
    assert!(engine.orders.pending_updates.borrow().is_empty());
    assert_eq!(
        engine.order_snapshot(id).unwrap().quantity(),
        Quantity::from("2.000")
    );
}

#[rstest]
#[case::accepted_parent(false, false)]
#[case::rejected_parent(true, false)]
#[case::child_listed_first(false, true)]
fn test_process_order_parks_oto_child_until_parent_rejected(
    #[case] parent_rejected: bool,
    #[case] child_first: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::builder()
            .use_reduce_only(false)
            .build(),
    );
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_cache.borrow_mut().update_order(&event).unwrap();
    }));

    let parent_id = ClientOrderId::from("PARK-PARENT");
    let child_id = ClientOrderId::from("PARK-CHILD");
    // A reduce-only parent is rejected because this engine does not support reduce-only
    let mut parent = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(parent_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("900.00"))
        .reduce_only(parent_rejected)
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .submit(true)
        .build();
    let mut child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1100.00"))
        .parent_order_id(parent_id)
        .submit(true)
        .build();

    for order in [&parent, &child] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    let account_id = AccountId::from("ACCOUNT-001");
    if child_first {
        engine.process_order(&mut child, account_id);
        engine.process_order(&mut parent, account_id);
    } else {
        engine.process_order(&mut parent, account_id);
        engine.process_order(&mut child, account_id);
    }

    assert_eq!(engine.order_exists(parent_id), !parent_rejected);
    assert!(!engine.order_exists(child_id));
    assert_eq!(
        engine
            .orders
            .pending_oto_order_ids
            .borrow()
            .contains(&child_id),
        !parent_rejected
    );
    assert_eq!(
        cache.borrow().order(&child_id).unwrap().status(),
        if parent_rejected {
            OrderStatus::Rejected
        } else {
            OrderStatus::Submitted
        }
    );
}

#[rstest]
fn test_process_order_rejects_oto_child_of_deferred_parent_rejection_until_purged() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::builder()
            .use_reduce_only(false)
            .build(),
    );
    // Defers events as an immediate-mode venue does until its command completes
    let deferred = Rc::new(RefCell::new(Vec::new()));
    let handler_deferred = deferred.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_deferred.borrow_mut().push(event);
    }));

    let parent_id = ClientOrderId::from("PARK-PARENT");
    let child_id = ClientOrderId::from("PARK-CHILD");
    // A reduce-only parent is rejected because this engine does not support reduce-only
    let mut parent = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(parent_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("900.00"))
        .reduce_only(true)
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .submit(true)
        .build();
    let mut child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("1100.00"))
        .parent_order_id(parent_id)
        .submit(true)
        .build();

    for order in [&parent, &child] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    let account_id = AccountId::from("ACCOUNT-001");
    engine.process_order(&mut parent, account_id);
    engine.process_order(&mut child, account_id);

    let events = deferred.borrow().clone();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| matches!(event, OrderEventAny::Rejected(_)))
    );
    assert_eq!(events[1].client_order_id(), child_id);
    assert!(
        !engine
            .orders
            .pending_oto_order_ids
            .borrow()
            .contains(&child_id)
    );

    // Kept until the cache shows the rejection
    engine.iterate(UnixNanos::from(1), AggressorSide::NoAggressor);
    assert!(
        engine
            .orders
            .rejected_oto_parent_ids
            .borrow()
            .contains(&parent_id)
    );

    for event in &events {
        cache.borrow_mut().update_order(event).unwrap();
    }

    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert!(engine.orders.rejected_oto_parent_ids.borrow().is_empty());
}

#[rstest]
#[case::cache_closed(false, false)]
#[case::cache_missing(true, false)]
#[case::engine_reset(false, true)]
fn test_deferred_canceled_oto_child_lifecycle(#[case] cache_missing: bool, #[case] reset: bool) {
    let (mut engine, _) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.support_contingent_orders = true;
    let child_id = ClientOrderId::from("OTO-CANCELED-CHILD");
    let child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("7.000"))
        .price(Price::from("1600.00"))
        .parent_order_id(ClientOrderId::from("OTO-PARENT"))
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(child.clone(), None, None, false)
        .unwrap();
    let deferred = Rc::new(RefCell::new(Vec::new()));
    let handler_deferred = deferred.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_deferred.borrow_mut().push(event);
    }));

    engine.cancel_order(&child, Some(false));
    for timestamp in [2, 3] {
        engine.iterate(UnixNanos::from(timestamp), AggressorSide::NoAggressor);
        assert_eq!(
            engine.cache.borrow().order(&child_id).unwrap().status(),
            OrderStatus::Submitted
        );
        assert_eq!(
            engine
                .orders
                .closed_oto_order_ids
                .borrow()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![child_id]
        );
    }

    let events = deferred.borrow().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type(), OrderEventType::Canceled);
    assert_eq!(events[0].client_order_id(), child_id);

    if reset {
        engine.reset();
    } else {
        if cache_missing {
            engine.cache.borrow_mut().reset();
            assert!(!engine.cache.borrow().order_exists(&child_id));
        } else {
            engine.cache.borrow_mut().update_order(&events[0]).unwrap();
            assert_eq!(
                engine.cache.borrow().order(&child_id).unwrap().status(),
                OrderStatus::Canceled
            );
        }

        engine.iterate(UnixNanos::from(4), AggressorSide::NoAggressor);
    }

    assert!(engine.orders.closed_oto_order_ids.borrow().is_empty());
}

#[rstest]
fn test_post_match_order_action_does_not_clone_closed_order() {
    let order = post_match_closed_limit_order();
    let clone_count = Cell::new(0);
    let action = post_match_order_action(&order, true, UnixNanos::from(1_u64), |order| {
        clone_count.set(clone_count.get() + 1);
        order.clone()
    });

    assert!(matches!(action, PostMatchOrderAction::RemoveClosed));
    assert_eq!(clone_count.get(), 0);
}

#[rstest]
fn test_post_match_order_action_clones_expired_gtd_order_once() {
    let order = post_match_gtd_limit_order();
    let clone_count = Cell::new(0);
    let action = post_match_order_action(&order, true, UnixNanos::from(10_u64), |order| {
        clone_count.set(clone_count.get() + 1);
        order.clone()
    });

    let PostMatchOrderAction::Expire(cloned) = action else {
        panic!("Expected expired action, was {action:?}");
    };

    assert_eq!(cloned.client_order_id(), order.client_order_id());
    assert_eq!(clone_count.get(), 1);
}

#[rstest]
fn test_post_match_order_action_clones_trailing_order_once() {
    let order = post_match_trailing_stop_order();
    let clone_count = Cell::new(0);
    let action = post_match_order_action(&order, true, UnixNanos::from(1_u64), |order| {
        clone_count.set(clone_count.get() + 1);
        order.clone()
    });

    let PostMatchOrderAction::UpdateTrailing(cloned) = action else {
        panic!("Expected trailing update action, was {action:?}");
    };

    assert_eq!(cloned.client_order_id(), order.client_order_id());
    assert_eq!(clone_count.get(), 1);
}

fn post_match_limit_order() -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt().id())
        .side(OrderSide::Buy)
        .price(Price::from("1500.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id(ClientOrderId::from("POST-MATCH-LIMIT"))
        .submit(true)
        .build()
}

fn post_match_closed_limit_order() -> OrderAny {
    let account_id = AccountId::from("SIM-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let mut order = post_match_limit_order();
    order
        .apply(TestOrderEventStubs::accepted(
            &order,
            account_id,
            venue_order_id,
        ))
        .unwrap();
    order
        .apply(TestOrderEventStubs::canceled(
            &order,
            account_id,
            Some(venue_order_id),
        ))
        .unwrap();
    order
}

fn post_match_gtd_limit_order() -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(crypto_perpetual_ethusdt().id())
        .side(OrderSide::Buy)
        .price(Price::from("1500.00"))
        .quantity(Quantity::from("1.000"))
        .time_in_force(TimeInForce::Gtd)
        .expire_time(UnixNanos::from(10_u64))
        .client_order_id(ClientOrderId::from("POST-MATCH-GTD"))
        .submit(true)
        .build()
}

fn post_match_trailing_stop_order() -> OrderAny {
    OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(crypto_perpetual_ethusdt().id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from("1510.00"))
        .trigger_type(TriggerType::BidAsk)
        .trailing_offset(Decimal::new(5, 0))
        .trailing_offset_type(TrailingOffsetType::Price)
        .client_order_id(ClientOrderId::from("POST-MATCH-TRAIL"))
        .submit(true)
        .build()
}

#[rstest]
fn test_fill_order_calculates_commission_from_fill_liquidity_side() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.0002), dec!(0.0004))).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    engine.set_event_handler(Rc::new(move |event| {
        events_handler.borrow_mut().push(event);
    }));

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    order.set_liquidity_side(LiquiditySide::Maker);
    engine
        .account_ids
        .insert(order.trader_id(), AccountId::from("ACCOUNT-001"));

    engine
        .fill_order(
            &order,
            Price::from("1500.00"),
            Quantity::from("1.000"),
            LiquiditySide::Taker,
            None,
            None,
        )
        .unwrap();

    let events = events.borrow();
    assert_eq!(events.len(), 1);

    let fill = match &events[0] {
        OrderEventAny::Filled(fill) => fill,
        event => panic!("Expected OrderFilled, was {event:?}"),
    };

    let commission = fill.commission.expect("expected commission");
    let expected_commission = fill.last_qty.as_decimal() * fill.last_px.as_decimal() * dec!(0.0004);

    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert_eq!(commission.currency, instrument.quote_currency());
    assert_eq!(commission.as_decimal(), expected_commission);
}

#[rstest]
fn test_fill_order_does_not_cache_filled_qty_when_fee_model_fails() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelHandle::new(FailingFeeModel),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    engine.set_event_handler(Rc::new(move |event| {
        events_handler.borrow_mut().push(event);
    }));

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    engine
        .account_ids
        .insert(order.trader_id(), AccountId::from("ACCOUNT-001"));

    let result = engine.fill_order(
        &order,
        Price::from("1500.00"),
        Quantity::from("1.000"),
        LiquiditySide::Taker,
        None,
        None,
    );

    assert!(result.is_err());
    assert_eq!(engine.cached_filled_qty_len(), 0);
    assert!(events.borrow().is_empty());
}

struct FailingFeeModel;

impl FeeModel for FailingFeeModel {
    fn get_commission(
        &self,
        _order: &OrderAny,
        _fill_quantity: Quantity,
        _fill_px: Price,
        _instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        Err(anyhow::anyhow!("fee model failed"))
    }
}

#[rstest]
fn test_l1_depth_skips_padding_for_last_quote_tracking() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );
    let mut bids = [BookOrder::default(); DEPTH10_LEN];
    let mut asks = [BookOrder::default(); DEPTH10_LEN];

    bids[1] = BookOrder::new(
        OrderSide::Buy,
        Price::from("1499.00"),
        Quantity::from("1.000"),
        1,
    );

    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("1500.00"),
        Quantity::from("1.000"),
        2,
    );

    let depth = OrderBookDepth::new(
        instrument.id(),
        bids,
        asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        0,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );
    engine.process_order_book_depth(&depth).unwrap();

    assert_eq!(engine.last_quote_bid, Some(Price::from("1499.00")));
    assert_eq!(engine.last_quote_ask, Some(Price::from("1500.00")));

    let depth_without_bid = OrderBookDepth::new(
        instrument.id(),
        [BookOrder::default(); DEPTH10_LEN],
        asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        1,
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
    );
    engine.process_order_book_depth(&depth_without_bid).unwrap();

    assert_eq!(engine.last_quote_bid, None);
    assert_eq!(engine.last_quote_ask, Some(Price::from("1500.00")));
}

#[rstest]
#[case::historical_gtc((None, TimeInForce::Gtc, "3.000", OrderStatus::Filled))]
#[case::historical_ioc((None, TimeInForce::Ioc, "3.000", OrderStatus::Filled))]
#[case::historical_fok((None, TimeInForce::Fok, "3.000", OrderStatus::Filled))]
#[case::custom_fok((Some("3.000"), TimeInForce::Fok, "3.000", OrderStatus::Filled))]
#[case::empty_gtc((Some("0.000"), TimeInForce::Gtc, "0.000", OrderStatus::Accepted))]
#[case::empty_ioc((Some("0.000"), TimeInForce::Ioc, "0.000", OrderStatus::Canceled))]
#[case::empty_fok((Some("0.000"), TimeInForce::Fok, "0.000", OrderStatus::Canceled))]
#[case::partial_gtc((Some("1.000"), TimeInForce::Gtc, "1.000", OrderStatus::PartiallyFilled))]
#[case::partial_ioc((Some("1.000"), TimeInForce::Ioc, "1.000", OrderStatus::Canceled))]
#[case::partial_fok((Some("1.000"), TimeInForce::Fok, "0.000", OrderStatus::Canceled))]
fn test_l2_custom_book_order_lifecycle(
    #[case] scenario: (Option<&str>, TimeInForce, &str, OrderStatus),
    #[values(OrderType::Market, OrderType::Limit)] order_type: OrderType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(false, true)] liquidity_consumption: bool,
    #[values(false, true)] use_market_order_acks: bool,
    #[values(false, true)] one_sided: bool,
) {
    let (custom_size, time_in_force, expected_qty, expected_status) = scenario;
    let (mut engine, events) = custom_book_engine(
        BookType::L2_MBP,
        liquidity_consumption,
        use_market_order_acks,
    );

    if one_sided {
        match side {
            OrderSide::Buy => engine.book.clear_bids(2, UnixNanos::from(2)),
            OrderSide::Sell => engine.book.clear_asks(2, UnixNanos::from(2)),
        }

        engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    }

    if let Some(size) = custom_size {
        set_custom_fill_book(&mut engine, size, false);
    }

    let price = Price::from(if side == OrderSide::Buy {
        "1501.00"
    } else {
        "1499.00"
    });

    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(engine.instrument.id())
        .side(side)
        .price(price)
        .quantity(Quantity::from("3.000"))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    if matches!((order_type, custom_size), (OrderType::Market, Some(_))) {
        assert!(
            engine
                .determine_market_fill_model_price_and_volume(&order)
                .unwrap()
                .1
        );
    }

    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let expected_qty = Quantity::from(expected_qty);

    let expected_status = match (order_type, expected_status, use_market_order_acks) {
        (OrderType::Market, OrderStatus::Accepted, false) => OrderStatus::Rejected,
        (OrderType::Market, OrderStatus::Accepted, true) => OrderStatus::Canceled,
        _ => expected_status,
    };

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.order_side, fill.last_px, fill.last_qty)),
            _ => None,
        })
        .collect();

    let expected_fills = if expected_qty.is_zero() {
        Vec::new()
    } else {
        vec![(side, price, expected_qty)]
    };

    assert_eq!(fills, expected_fills);
    let cached = engine
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(cached.status(), expected_status);
    assert_eq!(cached.filled_qty(), expected_qty);
    assert_eq!(cached.leaves_qty(), Quantity::from("3.000") - expected_qty);
    let historical_size = Some(Quantity::from("10.000"));
    let expected_sizes = match (one_sided, side) {
        (true, OrderSide::Buy) => (None, historical_size),
        (true, OrderSide::Sell) => (historical_size, None),
        (false, _) => (historical_size, historical_size),
    };

    assert_eq!(
        (engine.book.best_bid_size(), engine.book.best_ask_size()),
        expected_sizes,
    );

    if custom_size.is_some() {
        assert!(engine.bid_consumption.is_empty());
        assert!(engine.ask_consumption.is_empty());
    }

    if expected_qty.is_zero() {
        assert!(engine.orders.cached_filled_qty.borrow().is_empty());
        assert!(engine.pending_fills.is_empty());
        assert_eq!(
            engine
                .cache
                .borrow()
                .positions_total_count(None, None, None, None, None),
            0
        );
    }
}

#[rstest]
#[case::empty("0.000", false, "0.000")]
#[case::outside_limit("3.000", true, "0.000")]
#[case::partial("1.000", false, "1.000")]
fn test_l2_custom_book_limit_waits_for_replenishment(
    #[case] custom_size: &str,
    #[case] outside_limit: bool,
    #[case] initial_filled_qty: &str,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok)] time_in_force: TimeInForce,
    #[values(false, true)] liquidity_consumption: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, liquidity_consumption, false);
    set_custom_fill_book(&mut engine, custom_size, outside_limit);

    let price = Price::from(if side == OrderSide::Buy {
        "1501.00"
    } else {
        "1499.00"
    });

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .side(side)
        .price(price)
        .quantity(Quantity::from("3.000"))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    let id = order.client_order_id();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let initial_filled_qty = if time_in_force == TimeInForce::Fok {
        Quantity::from("0.000")
    } else {
        Quantity::from(initial_filled_qty)
    };

    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().filled_qty(),
        initial_filled_qty
    );
    events.borrow_mut().clear();

    set_custom_fill_book(&mut engine, "0.000", false);
    engine.iterate(UnixNanos::from(2), AggressorSide::NoAggressor);
    assert!(events.borrow().is_empty());
    assert_eq!(engine.order_exists(id), time_in_force == TimeInForce::Gtc);

    set_custom_fill_book(&mut engine, "3.000", false);
    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);

    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.price(), Some(price));
    assert!(engine.bid_consumption.is_empty());
    assert!(engine.ask_consumption.is_empty());

    if time_in_force == TimeInForce::Gtc {
        assert_eq!(cached.status(), OrderStatus::Filled);
        assert_eq!(cached.filled_qty(), Quantity::from("3.000"));
        let events = events.borrow();
        assert_eq!(events.len(), 1);

        let OrderEventAny::Filled(fill) = &events[0] else {
            panic!("Expected fill")
        };

        assert_eq!(fill.last_px, price);
        assert_eq!(fill.last_qty, Quantity::from("3.000") - initial_filled_qty);
    } else {
        assert_eq!(cached.status(), OrderStatus::Canceled);
        assert_eq!(cached.filled_qty(), initial_filled_qty);
        assert!(events.borrow().is_empty());
    }
}

#[rstest]
fn test_l2_custom_book_empty_market_before_acceptance(
    #[values(
        OrderStatus::Initialized,
        OrderStatus::Released,
        OrderStatus::Submitted
    )]
    status: OrderStatus,
    #[values(TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok)] time_in_force: TimeInForce,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(false, true)] liquidity_consumption: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, liquidity_consumption, false);
    set_custom_fill_book(&mut engine, "0.000", false);
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(engine.instrument.id())
        .side(side)
        .quantity(Quantity::from("3.000"))
        .time_in_force(time_in_force)
        .submit(status == OrderStatus::Submitted)
        .build();

    if status == OrderStatus::Released {
        order
            .apply(OrderEventAny::Released(OrderReleased::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                Price::from("1500.00"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            )))
            .unwrap();
    }

    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let expected_status = if time_in_force == TimeInForce::Gtc && status != OrderStatus::Released {
        OrderStatus::Rejected
    } else {
        OrderStatus::Canceled
    };

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        OrderEventAny::Rejected(_) | OrderEventAny::Canceled(_)
    ));
    let cache = engine.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.status(), expected_status);
    assert!(cached.filled_qty().is_zero());
    assert_eq!(cache.positions_total_count(None, None, None, None, None), 0);
    assert!(engine.bid_consumption.is_empty());
    assert!(engine.ask_consumption.is_empty());
    assert!(engine.orders.cached_filled_qty.borrow().is_empty());
}

fn set_custom_fill_book(engine: &mut OrderMatchingEngine, size: &str, outside_limit: bool) {
    let mut book = OrderBook::new(engine.instrument.id(), BookType::L2_MBP);
    let size = Quantity::from(size);
    if size.is_positive() {
        for (side, price) in [
            (
                OrderSide::Buy,
                if outside_limit { "1498.00" } else { "1499.00" },
            ),
            (
                OrderSide::Sell,
                if outside_limit { "1502.00" } else { "1501.00" },
            ),
        ] {
            book.add(
                BookOrder::new(side, Price::from(price), size, 0),
                0,
                1,
                UnixNanos::from(1),
            );
        }
    }

    engine.set_fill_model(FillModelHandle::new(TestBookFillModel { book }));
}

fn custom_book_engine(
    book_type: BookType,
    liquidity_consumption: bool,
    use_market_order_acks: bool,
) -> (OrderMatchingEngine, Rc<RefCell<Vec<OrderEventAny>>>) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        book_type,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig {
            liquidity_consumption,
            use_market_order_acks,
            ..Default::default()
        },
    );

    for (side, price) in [(OrderSide::Buy, "1499.00"), (OrderSide::Sell, "1501.00")] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side, Price::from(price), Quantity::from("10.000"), 0),
                RecordFlag::F_LAST as u8,
                1,
                UnixNanos::from(1),
                UnixNanos::from(1),
            ))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let saved_events = events.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if matches!(&event, OrderEventAny::Triggered(_)) {
            assert_eq!(
                cache
                    .borrow()
                    .order(&event.client_order_id())
                    .unwrap()
                    .status(),
                OrderStatus::Triggered
            );
        } else {
            cache.borrow_mut().update_order(&event).unwrap();
        }

        saved_events.borrow_mut().push(event);
    }));

    (engine, events)
}

struct TestBookFillModel {
    book: OrderBook,
}

impl FillModel for TestBookFillModel {
    fn is_limit_filled(&mut self) -> anyhow::Result<bool> {
        Ok(true)
    }

    fn is_slipped(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn get_orderbook_for_fill_simulation(
        &mut self,
        _instrument: &InstrumentAny,
        _order: &OrderAny,
        _best_bid: Option<Price>,
        _best_ask: Option<Price>,
    ) -> anyhow::Result<Option<OrderBook>> {
        Ok(Some(self.book.clone()))
    }
}

#[rstest]
fn test_canceled_fok_reverts_only_crossed_consumption_levels(
    #[values(OrderType::Market, OrderType::Limit)] order_type: OrderType,
    #[values(false, true)] reduce_only: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, true, false);
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            engine.instrument.id(),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1502.00"),
                Quantity::from("5.000"),
                0,
            ),
            RecordFlag::F_LAST as u8,
            2,
            UnixNanos::from(2),
            UnixNanos::from(2),
        ))
        .unwrap();

    // Prior consumption leaves 6 of 10 at 1501, and 1600 is never crossed
    let size = |qty: &str| Quantity::from(qty).raw();
    engine
        .ask_consumption
        .insert(Price::from("1600.00").raw(), (size("7.000"), size("3.000")));
    engine.ask_consumption.insert(
        Price::from("1501.00").raw(),
        (size("10.000"), size("4.000")),
    );
    let levels_before: Vec<_> = engine.ask_consumption.clone().into_iter().collect();

    // Crossed size is 11, below both the 15 ordered and the 12 reducible
    let position_id = reduce_only.then_some(PositionId::from("FOK-CONSUMPTION-POS"));
    if let Some(position_id) = position_id {
        let (opening, fill) = pending_position_fill(
            &engine.instrument,
            position_id,
            "FOK-CONSUMPTION-OPEN",
            OrderSide::Sell,
            "12.000",
        );
        let position = Position::new(&engine.instrument, fill.clone());
        engine
            .cache
            .borrow_mut()
            .add_order(opening, Some(position_id), None, false)
            .unwrap();
        engine
            .cache
            .borrow_mut()
            .update_order(&OrderEventAny::Filled(fill))
            .unwrap();
        engine
            .cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();
    }

    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(engine.instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("15.000"))
        .time_in_force(TimeInForce::Fok)
        .reduce_only(reduce_only)
        .submit(true);

    if order_type == OrderType::Limit {
        builder.price(Price::from("1502.00"));
    }

    let mut order = builder.build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), position_id, None, false)
        .unwrap();
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_)))
    );
    assert_eq!(
        engine
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Canceled
    );
    let levels_after: Vec<_> = engine.ask_consumption.clone().into_iter().collect();
    assert_eq!(levels_after, levels_before);
    assert!(engine.bid_consumption.is_empty());
    assert_eq!(engine.trade_consumption, 0);
}

#[rstest]
fn test_fee_underlying_price_uses_valid_cached_greeks_price() {
    let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit(
        3,
        1,
        Price::from("0.001"),
        Quantity::from("0.1"),
    ));
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_option_greeks(OptionGreeks {
        instrument_id: instrument.id(),
        underlying_price: Some(50_000.0),
        ..Default::default()
    });

    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );

    let price = engine
        .fee_underlying_price()
        .unwrap()
        .expect("expected underlying price");

    assert_eq!(price.precision, FIXED_PRECISION);
    assert_eq!(price.as_decimal(), Decimal::from(50_000));
}

#[rstest]
fn test_fee_underlying_price_rejects_invalid_cached_greeks_price() {
    let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit(
        3,
        1,
        Price::from("0.001"),
        Quantity::from("0.1"),
    ));
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_option_greeks(OptionGreeks {
        instrument_id: instrument.id(),
        underlying_price: Some(f64::NAN),
        ..Default::default()
    });

    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );

    let error = engine.fee_underlying_price().unwrap_err();

    assert_eq!(
        error,
        CorrectnessError::InvalidValue {
            param: "value".to_string(),
            value: "NaN".to_string(),
            type_name: "f64",
        }
    );
}

#[rstest]
fn test_bar_tick_sizes_divisible() {
    // precision=3, units=100_000: exactly divisible by 4, no rounding.
    let volume = Quantity::from("100.000");
    let increment = Quantity::from("0.001");
    let sizes = BarTickSizes::from_volume(volume, increment);
    assert_eq!(sizes.open, Quantity::from("25.000"));
    assert_eq!(sizes.high, Quantity::from("25.000"));
    assert_eq!(sizes.low, Quantity::from("25.000"));
    assert_eq!(sizes.close, Quantity::from("25.000"));
    assert_valid_bar_tick_sizes(volume, increment);
}

#[rstest]
fn test_bar_tick_sizes_indivisible_with_remainder() {
    // precision=2, units=5: quarter_units=1, remainder=1; close carries 2 units.
    let volume = Quantity::from("0.05");
    let increment = Quantity::from("0.01");
    let sizes = BarTickSizes::from_volume(volume, increment);
    assert_eq!(sizes.open, Quantity::from("0.01"));
    assert_eq!(sizes.high, Quantity::from("0.01"));
    assert_eq!(sizes.low, Quantity::from("0.01"));
    assert_eq!(sizes.close, Quantity::from("0.02"));
    assert_valid_bar_tick_sizes(volume, increment);
    assert_eq!(
        sizes.open.raw() + sizes.high.raw() + sizes.low.raw() + sizes.close.raw(),
        volume.raw()
    );
}

#[rstest]
#[case("1", "0", "0", "0", "1")]
#[case("2", "0", "1", "1", "0")]
#[case("3", "1", "1", "1", "0")]
fn test_bar_tick_sizes_units_less_than_four_preserves_volume(
    #[case] volume: &str,
    #[case] open_size: &str,
    #[case] high_size: &str,
    #[case] low_size: &str,
    #[case] close_size: &str,
) {
    let volume = Quantity::from(volume);
    let increment = Quantity::from("1");
    let sizes = BarTickSizes::from_volume(volume, increment);

    assert_eq!(sizes.open, Quantity::from(open_size));
    assert_eq!(sizes.high, Quantity::from(high_size));
    assert_eq!(sizes.low, Quantity::from(low_size));
    assert_eq!(sizes.close, Quantity::from(close_size));
    assert_valid_bar_tick_sizes(volume, increment);
    assert_eq!(
        sizes.open.raw() + sizes.high.raw() + sizes.low.raw() + sizes.close.raw(),
        volume.raw()
    );
}

#[rstest]
fn test_bar_tick_sizes_zero_volume_remains_zero() {
    let volume = Quantity::zero(3);
    let increment = Quantity::from("0.001");
    let sizes = BarTickSizes::from_volume(volume, increment);
    assert_eq!(sizes.open, Quantity::zero(3));
    assert_eq!(sizes.high, Quantity::zero(3));
    assert_eq!(sizes.low, Quantity::zero(3));
    assert_eq!(sizes.close, Quantity::zero(3));
    assert_valid_bar_tick_sizes(volume, increment);
}

#[rstest]
fn test_bar_tick_sizes_rounds_down_to_size_increment() {
    let volume = Quantity::from("1.07");
    let increment = Quantity::from("0.10");
    let sizes = BarTickSizes::from_volume(volume, increment);
    assert_eq!(sizes.open, Quantity::from("0.20"));
    assert_eq!(sizes.high, Quantity::from("0.20"));
    assert_eq!(sizes.low, Quantity::from("0.20"));
    assert_eq!(sizes.close, Quantity::from("0.40"));
    assert_valid_bar_tick_sizes(volume, increment);
}

#[rstest]
fn test_bar_tick_sizes_at_fixed_precision() {
    // When volume.precision == FIXED_PRECISION the scale is 1 and the formula
    // degenerates to a plain raw-space quartering.
    let units: QuantityRaw = 17;
    let volume = Quantity::from_raw(units, FIXED_PRECISION);
    let increment = Quantity::from_raw(1, FIXED_PRECISION);
    let sizes = BarTickSizes::from_volume(volume, increment);
    assert_eq!(sizes.open.raw(), 4);
    assert_eq!(sizes.high.raw(), 4);
    assert_eq!(sizes.low.raw(), 4);
    assert_eq!(sizes.close.raw(), 5);
    assert_valid_bar_tick_sizes(volume, increment);
}

fn get_queue_engine(
    instrument: InstrumentAny,
    book_type: BookType,
) -> (OrderMatchingEngine, Rc<RefCell<Cache>>) {
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let config = OrderMatchingEngineConfig {
        trade_execution: true,
        queue_position: true,
        ..Default::default()
    };

    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        book_type,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        Rc::clone(&cache),
        config,
    );

    let handler_cache = Rc::clone(&cache);
    engine.set_event_handler(Rc::new(move |event: OrderEventAny| {
        if let Ok(mut cache) = handler_cache.try_borrow_mut() {
            let _ = cache.update_order(&event);
        }
    }));

    (engine, cache)
}

fn get_l3_queue_engine(instrument: InstrumentAny) -> (OrderMatchingEngine, Rc<RefCell<Cache>>) {
    get_queue_engine(instrument, BookType::L3_MBO)
}

fn assert_l3_queue_synced(engine: &OrderMatchingEngine) {
    for (client_order_id, orders_ahead) in &engine.queue_ahead_orders {
        let set_sum: QuantityRaw = orders_ahead.values().sum();
        let counter = engine
            .queue_ahead_total
            .get(client_order_id)
            .map_or(0, |&(_, ahead_raw)| ahead_raw);
        assert_eq!(
            set_sum, counter,
            "tracked orders out of sync with quantity-ahead counter for {client_order_id}",
        );
    }

    for (client_order_id, price_raw) in &engine.queue_pending {
        assert!(
            engine
                .queue_ids_by_price
                .get(price_raw)
                .is_some_and(|ids| ids.contains(client_order_id)),
            "pending order {client_order_id} missing from price index",
        );
    }

    for (client_order_id, (price_raw, _)) in &engine.queue_ahead_total {
        assert!(
            engine
                .queue_ids_by_price
                .get(price_raw)
                .is_some_and(|ids| ids.contains(client_order_id)),
            "tracked order {client_order_id} missing from price index",
        );
    }

    for (price_raw, client_order_ids) in &engine.queue_ids_by_price {
        for client_order_id in client_order_ids {
            let pending_at_price = engine.queue_pending.get(client_order_id) == Some(price_raw);
            let tracked_at_price = engine
                .queue_ahead_total
                .get(client_order_id)
                .is_some_and(|(tracked_price_raw, _)| tracked_price_raw == price_raw);
            assert!(
                pending_at_price || tracked_at_price,
                "price index contains stale order {client_order_id}",
            );
        }
    }
}

#[rstest]
fn test_reset_clears_queue_positions() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, _cache) = get_l3_queue_engine(instrument);
    let price = Price::from("100.00");
    let client_order_id = ClientOrderId::from("O-RESET-QUEUE");

    rest_l3_queue_order(&mut engine, price, 1, client_order_id);

    assert!(engine.queue_ahead_total.contains_key(&client_order_id));
    assert!(engine.queue_ahead_orders.contains_key(&client_order_id));
    assert!(
        engine
            .queue_ids_by_price
            .get(&price.raw())
            .is_some_and(|ids| ids.contains(&client_order_id)),
    );

    engine.reset();

    assert!(engine.queue_pending.is_empty());
    assert!(engine.queue_ahead_total.is_empty());
    assert!(engine.queue_ahead_orders.is_empty());
    assert!(engine.queue_excess.is_empty());
    assert!(engine.queue_ids_by_price.is_empty());
}

#[rstest]
fn test_cancel_removes_queue_position() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, _cache) = get_l3_queue_engine(instrument);
    let price = Price::from("100.00");
    let order = rest_l3_queue_order(&mut engine, price, 1, ClientOrderId::from("O-CANCEL-QUEUE"));
    let client_order_id = order.client_order_id();

    assert!(engine.queue_ahead_total.contains_key(&client_order_id));
    assert!(engine.queue_ahead_orders.contains_key(&client_order_id));
    assert!(
        engine
            .queue_ids_by_price
            .get(&price.raw())
            .is_some_and(|ids| ids.contains(&client_order_id)),
    );

    engine.cancel_order(&order, None);

    assert!(!engine.queue_pending.contains_key(&client_order_id));
    assert!(!engine.queue_ahead_total.contains_key(&client_order_id));
    assert!(!engine.queue_ahead_orders.contains_key(&client_order_id));
    assert!(!engine.queue_excess.contains_key(&client_order_id));
    assert!(!engine.queue_ids_by_price.contains_key(&price.raw()));
}

#[rstest]
fn test_modify_reindexes_queue_position() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, _cache) = get_l3_queue_engine(instrument);
    let old_price = Price::from("100.00");
    let new_price = Price::from("101.00");
    let client_order_id = ClientOrderId::from("O-MODIFY-QUEUE");
    let order = rest_l3_queue_order(&mut engine, old_price, 1, client_order_id);

    let new_level = OrderBookDelta::new(
        engine.instrument.id(),
        BookAction::Add,
        BookOrder::new(OrderSide::Sell, new_price, Quantity::from("10.000"), 2),
        0,
        2,
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    engine.process_order_book_delta(&new_level).unwrap();

    let command = ModifyOrder::new(
        order.trader_id(),
        None,
        order.strategy_id(),
        order.instrument_id(),
        client_order_id,
        order.venue_order_id(),
        None,
        Some(new_price),
        None,
        UUID4::new(),
        UnixNanos::from(3),
        None,
        None,
    );
    engine.process_modify(&command, AccountId::from("SIM-001"));

    assert!(!engine.queue_ids_by_price.contains_key(&old_price.raw()));
    assert_eq!(
        engine
            .queue_ids_by_price
            .get(&new_price.raw())
            .map(|ids| ids.iter().copied().collect::<Vec<_>>()),
        Some(vec![client_order_id]),
    );
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(new_price.raw(), Quantity::from("10.000").raw())),
    );
    assert_eq!(
        engine
            .queue_ahead_orders
            .get(&client_order_id)
            .map(|orders| orders.keys().copied().collect::<Vec<_>>()),
        Some(vec![2]),
    );
}

#[rstest]
fn test_snapshot_rebases_l2_queue_position_after_size_decrease() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let (mut engine, cache) = get_queue_engine(instrument, BookType::L2_MBP);

    let initial = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("10.000"),
            0,
        ),
        0,
        1,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );
    engine.process_order_book_delta(&initial).unwrap();

    let client_order_id = ClientOrderId::from("O-SNAPSHOT-DECREASE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id(client_order_id)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("10.000").raw())),
    );

    let clear = OrderBookDelta::clear(
        instrument_id,
        2,
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
    );
    engine.process_order_book_delta(&clear).unwrap();
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("10.000").raw())),
        "partial snapshot must not discard the old queue estimate",
    );

    let snapshot = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("8.000"),
            0,
        ),
        RecordFlag::F_LAST as u8,
        2,
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
    );
    engine.process_order_book_delta(&snapshot).unwrap();

    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("8.000").raw())),
    );
    assert!(cache.borrow().order(&client_order_id).is_some());
}

#[rstest]
fn test_snapshot_rebase_does_not_increase_l2_queue_position() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let (mut engine, _cache) = get_queue_engine(instrument, BookType::L2_MBP);

    let initial = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("10.000"),
            0,
        ),
        0,
        1,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );
    engine.process_order_book_delta(&initial).unwrap();

    let client_order_id = ClientOrderId::from("O-SNAPSHOT-INCREASE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id(client_order_id)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    let snapshot = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("15.000"),
            0,
        ),
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
        2,
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
    );
    engine.process_order_book_delta(&snapshot).unwrap();

    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("10.000").raw())),
    );
}

#[rstest]
fn test_depth_rebases_l2_queue_position() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let (mut engine, _cache) = get_queue_engine(instrument, BookType::L2_MBP);

    let mut asks = [BookOrder::default(); DEPTH10_LEN];

    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.00"),
        Quantity::from("10.000"),
        0,
    );

    let initial = OrderBookDepth::new(
        instrument_id,
        [BookOrder::default(); DEPTH10_LEN],
        asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        1,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    );
    engine.process_order_book_depth(&initial).unwrap();

    let client_order_id = ClientOrderId::from("O-DEPTH-REBASE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id(client_order_id)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.00"),
        Quantity::from("8.000"),
        0,
    );

    let replacement = OrderBookDepth::new(
        instrument_id,
        [BookOrder::default(); DEPTH10_LEN],
        asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        2,
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
    );
    engine.process_order_book_depth(&replacement).unwrap();

    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("8.000").raw())),
    );
}

#[rstest]
fn test_snapshot_rebases_each_l3_order_independently() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let (mut engine, _cache) = get_l3_queue_engine(instrument);

    for (order_id, sequence) in [(1, 1), (2, 2)] {
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("5.000"),
                order_id,
            ),
            0,
            sequence,
            UnixNanos::from(sequence),
            UnixNanos::from(sequence),
        );
        engine.process_order_book_delta(&delta).unwrap();
    }

    let client_order_id = ClientOrderId::from("O-SNAPSHOT-L3");
    let order = rest_l3_queue_order(&mut engine, Price::from("100.00"), 3, client_order_id);
    assert_eq!(
        engine.queue_ahead_orders[&client_order_id]
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![1, 2, 3],
    );
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("20.000").raw())),
    );

    let snapshot = OrderBookDeltas::new(
        instrument_id,
        vec![
            OrderBookDelta::clear(
                instrument_id,
                4,
                UnixNanos::from(4_u64),
                UnixNanos::from(4_u64),
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("100.00"),
                    Quantity::from("10.000"),
                    1,
                ),
                RecordFlag::F_SNAPSHOT as u8,
                4,
                UnixNanos::from(4_u64),
                UnixNanos::from(4_u64),
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("100.00"),
                    Quantity::from("5.000"),
                    2,
                ),
                RecordFlag::F_LAST as u8,
                4,
                UnixNanos::from(4_u64),
                UnixNanos::from(4_u64),
            ),
        ],
    );
    engine.process_order_book_deltas(&snapshot).unwrap();

    assert_eq!(
        engine.queue_ahead_orders[&client_order_id]
            .iter()
            .map(|(&order_id, &size)| (order_id, size))
            .collect::<Vec<_>>(),
        vec![
            (1, Quantity::from("5.000").raw()),
            (2, Quantity::from("5.000").raw())
        ],
    );
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("10.000").raw())),
    );

    let delete_a = OrderBookDelta::new(
        instrument_id,
        BookAction::Delete,
        BookOrder::new(
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("10.000"),
            1,
        ),
        0,
        5,
        UnixNanos::from(5_u64),
        UnixNanos::from(5_u64),
    );
    engine.process_order_book_delta(&delete_a).unwrap();

    assert_eq!(
        engine.queue_ahead_orders[&client_order_id]
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![2],
    );
    assert_eq!(
        engine.queue_ahead_total.get(&client_order_id),
        Some(&(Price::from("100.00").raw(), Quantity::from("5.000").raw())),
    );
    assert_eq!(order.client_order_id(), client_order_id);
}

#[rstest]
fn test_queue_price_index_filters_other_prices() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, _cache) = get_l3_queue_engine(instrument);
    let target_price = Price::from("100.00");
    let other_price = Price::from("101.00");
    let target_id = ClientOrderId::from("O-QUEUE-TARGET");
    let other_id = ClientOrderId::from("O-QUEUE-OTHER");

    rest_l3_queue_order(&mut engine, target_price, 1, target_id);
    rest_l3_queue_order(&mut engine, other_price, 2, other_id);

    let indexed_ids = engine.take_queue_ids_at_price(target_price.raw());

    assert_eq!(indexed_ids, vec![target_id]);
    assert!(
        engine
            .queue_ids_by_price
            .get(&other_price.raw())
            .is_some_and(|ids| ids.contains(&other_id)),
    );
}

fn rest_l3_queue_order(
    engine: &mut OrderMatchingEngine,
    price: Price,
    sequence: u64,
    client_order_id: ClientOrderId,
) -> OrderAny {
    let instrument_id = engine.instrument.id();

    let delta = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(OrderSide::Sell, price, Quantity::from("10.000"), sequence),
        0,
        sequence,
        UnixNanos::from(sequence),
        UnixNanos::from(sequence),
    );
    engine.process_order_book_delta(&delta).unwrap();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .price(price)
        .quantity(Quantity::from("5.000"))
        .client_order_id(client_order_id)
        .submit(true)
        .build();
    process_cached_order(engine, &mut order, AccountId::from("SIM-001"));

    order
}

#[derive(Debug, Clone, Copy)]
enum QueueEvent {
    Add { id: OrderId, size: u64 },
    Update { id: OrderId, size: u64 },
    MoveAway { id: OrderId },
    Delete { id: OrderId },
    Trade { size: u64, aggressor: u8 },
    AggregateCap { size: u64 },
    AggregateDelete,
    RestOrder,
}

fn granular_queue_event() -> impl Strategy<Value = QueueEvent> {
    prop_oneof![
        3 => (1u64..=6, 1u64..=9).prop_map(|(id, size)| QueueEvent::Add { id, size }),
        3 => (1u64..=6, 1u64..=9).prop_map(|(id, size)| QueueEvent::Update { id, size }),
        1 => (1u64..=6).prop_map(|id| QueueEvent::MoveAway { id }),
        2 => (1u64..=6).prop_map(|id| QueueEvent::Delete { id }),
        2 => Just(QueueEvent::RestOrder),
    ]
}

fn any_queue_event() -> impl Strategy<Value = QueueEvent> {
    prop_oneof![
        5 => granular_queue_event(),
        3 => (1u64..=9, 0u8..3).prop_map(|(size, aggressor)| QueueEvent::Trade {
            size,
            aggressor,
        }),
        1 => (1u64..=9).prop_map(|size| QueueEvent::AggregateCap { size }),
        1 => Just(QueueEvent::AggregateDelete),
    ]
}

// Drives generated events through an L3 queue_position engine; the
// shadow id maps sanitize the feed to what real MBO feeds guarantee
struct L3QueueSim {
    engine: OrderMatchingEngine,
    account_id: AccountId,
    live_main: HashMap<OrderId, u64>,
    live_away: HashSet<OrderId>,
    rest_snapshots: HashMap<ClientOrderId, HashSet<OrderId>>,
    rested: usize,
    sequence: u64,
}

impl L3QueueSim {
    const MAIN_PRICE: &'static str = "100.00";
    const AWAY_PRICE: &'static str = "101.00";

    fn new() -> Self {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let (engine, _cache) = get_l3_queue_engine(instrument);

        Self {
            engine,
            account_id: AccountId::from("SIM-001"),
            live_main: HashMap::new(),
            live_away: HashSet::new(),
            rest_snapshots: HashMap::new(),
            rested: 0,
            sequence: 0,
        }
    }

    fn quantity(size: u64) -> Quantity {
        Quantity::from(format!("{size}.000").as_str())
    }

    fn process_delta(
        &mut self,
        action: BookAction,
        price: &str,
        size: u64,
        order_id: OrderId,
        flags: u8,
    ) {
        self.sequence += 1;

        let delta = OrderBookDelta::new(
            self.engine.instrument.id(),
            action,
            BookOrder::new(
                OrderSide::Sell,
                Price::from(price),
                Self::quantity(size),
                order_id,
            ),
            flags,
            self.sequence,
            UnixNanos::from(self.sequence),
            UnixNanos::from(self.sequence),
        );
        self.engine.process_order_book_delta(&delta).unwrap();
    }

    fn apply(&mut self, event: QueueEvent) {
        match event {
            QueueEvent::Add { id, size } => {
                if self.live_main.contains_key(&id) || self.live_away.contains(&id) {
                    return;
                }

                self.process_delta(BookAction::Add, Self::MAIN_PRICE, size, id, 0);
                self.live_main.insert(id, size);
            }
            QueueEvent::Update { id, size } => {
                if !self.live_main.contains_key(&id) {
                    return;
                }

                self.process_delta(BookAction::Update, Self::MAIN_PRICE, size, id, 0);
                self.live_main.insert(id, size);
            }
            QueueEvent::MoveAway { id } => {
                let Some(size) = self.live_main.remove(&id) else {
                    return;
                };

                self.process_delta(BookAction::Update, Self::AWAY_PRICE, size, id, 0);
                self.live_away.insert(id);
            }
            QueueEvent::Delete { id } => {
                if let Some(size) = self.live_main.remove(&id) {
                    self.process_delta(BookAction::Delete, Self::MAIN_PRICE, size, id, 0);
                } else if self.live_away.remove(&id) {
                    self.process_delta(BookAction::Delete, Self::AWAY_PRICE, 1, id, 0);
                } else {
                    // Unknown id exercises the ignore path
                    self.process_delta(BookAction::Delete, Self::MAIN_PRICE, 1, id, 0);
                }

                // A later Add reusing this id is a new order, not the
                // snapshot-time one (real feeds never reuse ids)
                for snapshot_ids in self.rest_snapshots.values_mut() {
                    snapshot_ids.remove(&id);
                }
            }
            QueueEvent::Trade { size, aggressor } => {
                self.sequence += 1;

                let aggressor_side = match aggressor {
                    0 => AggressorSide::Buy,
                    1 => AggressorSide::Sell,
                    _ => AggressorSide::NoAggressor,
                };

                let trade = TradeTick::new(
                    self.engine.instrument.id(),
                    Price::from(Self::MAIN_PRICE),
                    Self::quantity(size),
                    aggressor_side,
                    TradeId::new(format!("T-{}", self.sequence).as_str()),
                    UnixNanos::from(self.sequence),
                    UnixNanos::from(self.sequence),
                );
                self.engine.process_trade_tick(&trade);
            }
            QueueEvent::AggregateCap { size } => {
                self.process_delta(
                    BookAction::Update,
                    Self::MAIN_PRICE,
                    size,
                    0,
                    RecordFlag::F_MBP as u8,
                );
            }
            QueueEvent::AggregateDelete => {
                self.process_delta(
                    BookAction::Delete,
                    Self::MAIN_PRICE,
                    1,
                    0,
                    RecordFlag::F_MBP as u8,
                );
            }
            QueueEvent::RestOrder => {
                if self.rested >= 3 {
                    return;
                }

                self.rested += 1;
                let mut order = OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(self.engine.instrument.id())
                    .side(OrderSide::Sell)
                    .price(Price::from(Self::MAIN_PRICE))
                    .quantity(Self::quantity(5))
                    .client_order_id(ClientOrderId::from(
                        format!("O-PROP-{}", self.rested).as_str(),
                    ))
                    .submit(true)
                    .build();
                process_cached_order(&mut self.engine, &mut order, self.account_id);

                assert!(
                    self.engine
                        .queue_ahead_orders
                        .contains_key(&order.client_order_id()),
                    "L3 snapshot must track the resting order",
                );

                self.rest_snapshots.insert(
                    order.client_order_id(),
                    self.live_main.keys().copied().collect(),
                );
            }
        }
    }

    // Without trades or aggregate rows, tracked orders must mirror the book
    // exactly, and equal the rest-time snapshot ids still at the level
    fn assert_tracked_orders_match_book(&self) {
        let level: HashMap<OrderId, QuantityRaw> = self
            .engine
            .book
            .get_orders_at_level(Price::from(Self::MAIN_PRICE), OrderSide::Buy)
            .iter()
            .map(|order| (order.order_id, order.size.raw()))
            .collect();

        for (client_order_id, orders_ahead) in &self.engine.queue_ahead_orders {
            for (order_id, size_raw) in orders_ahead {
                let book_size = level.get(order_id).copied().unwrap_or_else(|| {
                    panic!("tracked order {order_id} for {client_order_id} not in book level")
                });

                assert_eq!(
                    book_size, *size_raw,
                    "tracked size diverged from book for order {order_id}",
                );
            }

            let tracked: HashSet<OrderId> = orders_ahead.keys().copied().collect();
            let expected: HashSet<OrderId> = self.rest_snapshots[client_order_id]
                .iter()
                .filter(|id| self.live_main.contains_key(id))
                .copied()
                .collect();
            assert_eq!(
                tracked, expected,
                "tracked set incomplete or stale for {client_order_id}",
            );
        }
    }
}

#[rstest]
fn prop_test_l3_queue_tracking_stays_synced_with_counter() {
    proptest!(|(events in prop::collection::vec(any_queue_event(), 1..=80))| {
        let mut sim = L3QueueSim::new();
        for event in events {
            sim.apply(event);
            assert_l3_queue_synced(&sim.engine);
        }
    });
}

#[rstest]
fn prop_test_l3_queue_tracking_mirrors_book_without_trades() {
    proptest!(|(events in prop::collection::vec(granular_queue_event(), 1..=80))| {
        let mut sim = L3QueueSim::new();
        for event in events {
            sim.apply(event);
            assert_l3_queue_synced(&sim.engine);
            sim.assert_tracked_orders_match_book();
        }
    });
}

// Replays real GLBX MBO flow (records 9150..10650 of
// test_data/databento/esh4-glbx-mdp3-20231225.mbo.dbn.zst as JSON),
// joining the touch periodically; the mid-stream start also exercises
// unseen-id ignore paths
#[rstest]
fn test_l3_queue_position_replay_databento_mbo_stays_synced() {
    let json = include_str!("../../../../test_data/databento/esh4-glbx-mdp3-20231225.mbo.json");
    let records: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
    assert!(records.len() > 1000);

    let instrument = InstrumentAny::FuturesContract(futures_contract_es(None, None));
    let instrument_id = instrument.id();
    let (mut engine, cache) = get_l3_queue_engine(instrument);
    let account_id = AccountId::from("SIM-001");

    let mut rested = 0usize;
    let mut trades = 0usize;

    for (index, record) in records.iter().enumerate() {
        match record.get("type").and_then(serde_json::Value::as_str) {
            Some("OrderBookDelta") => {
                let mut delta: OrderBookDelta = serde_json::from_value(record.clone()).unwrap();
                delta.instrument_id = instrument_id;
                engine.process_order_book_delta(&delta).unwrap();
            }
            Some("TradeTick") => {
                let mut trade: TradeTick = serde_json::from_value(record.clone()).unwrap();
                trade.instrument_id = instrument_id;
                engine.process_trade_tick(&trade);
                trades += 1;
            }
            other => panic!("unexpected record type {other:?}"),
        }

        if index % 150 == 100 {
            let (side, price) = if rested.is_multiple_of(2) {
                (OrderSide::Sell, engine.book.best_ask_price())
            } else {
                (OrderSide::Buy, engine.book.best_bid_price())
            };

            if let Some(price) = price {
                rested += 1;
                let mut order = OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(instrument_id)
                    .side(side)
                    .price(price)
                    .quantity(Quantity::from("1"))
                    .client_order_id(ClientOrderId::from(format!("O-MBO-{rested}").as_str()))
                    .submit(true)
                    .build();
                engine.process_order(&mut order, account_id);

                // A crossed mid-stream book can fill a joined order on
                // arrival; only open orders are tracked
                let is_open = cache
                    .borrow()
                    .order(&order.client_order_id())
                    .is_some_and(|order| order.is_open());

                if is_open {
                    assert!(
                        engine
                            .queue_ahead_orders
                            .contains_key(&order.client_order_id()),
                        "L3 snapshot must track the resting order",
                    );
                }
            }
        }

        assert_l3_queue_synced(&engine);
    }

    assert!(rested >= 5, "replay must exercise resting orders");
    assert!(trades >= 50, "replay must exercise trade interleavings");
}

#[rstest]
#[case::buy(OrderSide::Buy, AggressorSide::Sell)]
#[case::sell(OrderSide::Sell, AggressorSide::Buy)]
fn test_l1_partial_queue_trades_preserve_remaining_ahead(
    #[case] side: OrderSide,
    #[case] aggressor: AggressorSide,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        1.into(),
        1.into(),
    ));

    let price = Price::from(if side == OrderSide::Buy {
        "100.00"
    } else {
        "101.00"
    });

    let id = ClientOrderId::from("QUEUE-PARTIAL");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(side)
        .price(price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );

    for (ts, size, ahead) in [(2, "4.000", "6.000"), (3, "3.000", "3.000")] {
        engine.process_trade_tick(&TradeTick::new(
            instrument.id(),
            price,
            Quantity::from(size),
            aggressor,
            TradeId::from(format!("T-{ts}")),
            ts.into(),
            ts.into(),
        ));
        assert_eq!(
            engine.queue_ahead_total[&id],
            (price.raw(), Quantity::from(ahead).raw())
        );
        let cache = cache.borrow();
        let resting = cache.order(&id).unwrap();
        assert_eq!(resting.status(), OrderStatus::Accepted);
        assert_eq!(resting.filled_qty(), Quantity::from("0.000"));
        assert_eq!(resting.leaves_qty(), Quantity::from("2.000"));
    }

    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        price,
        Quantity::from("5.000"),
        aggressor,
        TradeId::from("T-4"),
        4.into(),
        4.into(),
    ));
    assert_eq!(
        cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(
        cache.borrow().order(&id).unwrap().filled_qty(),
        Quantity::from("2.000")
    );
}

#[rstest]
#[case::at_bid(OrderSide::Buy, "100.00", "5.000", "8.000", "1.000")]
#[case::at_ask(OrderSide::Sell, "101.00", "5.000", "8.000", "1.000")]
#[case::inside_buy(OrderSide::Buy, "100.50", "0.000", "3.000", "1.000")]
#[case::inside_sell(OrderSide::Sell, "100.50", "0.000", "3.000", "1.000")]
#[case::exhausted_bid(OrderSide::Buy, "100.00", "5.000", "7.000", "0.000")]
#[case::exhausted_ask(OrderSide::Sell, "101.00", "5.000", "7.000", "0.000")]
fn test_l1_queue_orders_share_exact_trade_budget(
    #[case] side: OrderSide,
    #[case] limit: &str,
    #[case] ahead: &str,
    #[case] trade_size: &str,
    #[case] second_filled: &str,
    #[values(false, true)] liquidity_consumption: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);
    engine.config.liquidity_consumption = liquidity_consumption;
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("5.000"),
        Quantity::from("5.000"),
        1.into(),
        1.into(),
    ));
    let price = Price::from(limit);
    let ids = [
        ClientOrderId::from("QUEUE-FIRST"),
        ClientOrderId::from("QUEUE-SECOND"),
    ];

    for id in ids {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(id)
            .side(side)
            .price(price)
            .quantity(Quantity::from("2.000"))
            .submit(true)
            .build();
        process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
        assert_eq!(
            cache.borrow().order(&id).unwrap().status(),
            OrderStatus::Accepted
        );
        assert_eq!(
            engine.queue_ahead_total[&id],
            (price.raw(), Quantity::from(ahead).raw())
        );
    }

    let aggressor = if side == OrderSide::Buy {
        AggressorSide::Sell
    } else {
        AggressorSide::Buy
    };

    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        price,
        Quantity::from(trade_size),
        aggressor,
        TradeId::from("QUEUE-TRADE"),
        2.into(),
        2.into(),
    ));
    let cache = cache.borrow();
    let first = cache.order(&ids[0]).unwrap();
    let second = cache.order(&ids[1]).unwrap();
    assert_eq!(first.status(), OrderStatus::Filled);
    assert_eq!(first.filled_qty(), Quantity::from("2.000"));
    assert_eq!(first.leaves_qty(), Quantity::from("0.000"));
    let second_filled = Quantity::from(second_filled);
    assert_eq!(
        second.status(),
        if second_filled.is_zero() {
            OrderStatus::Accepted
        } else {
            OrderStatus::PartiallyFilled
        }
    );
    assert_eq!(second.filled_qty(), second_filled);
    assert_eq!(second.leaves_qty(), Quantity::from("2.000") - second_filled);
}

#[rstest]
fn test_seller_trade_seeds_every_crossed_bid_level() {
    let (mut engine, _) = custom_book_engine(BookType::L2_MBP, true, false);
    engine.book.clear(1, 1.into());
    for (price, size, id) in [("100.00", "2.000", 1), ("99.00", "3.000", 2)] {
        engine.book.add(
            BookOrder::new(OrderSide::Buy, Price::from(price), Quantity::from(size), id),
            0,
            1,
            1.into(),
        );
    }

    engine.seed_trade_consumption(
        Price::from("99.00").raw(),
        Quantity::from("4.000").raw(),
        2.into(),
        AggressorSide::Sell,
    );
    assert_eq!(engine.bid_consumption.len(), 2);
    assert_eq!(
        engine.bid_consumption[&Price::from("100.00").raw()],
        (Quantity::from("2.000").raw(), Quantity::from("2.000").raw())
    );
    assert_eq!(
        engine.bid_consumption[&Price::from("99.00").raw()],
        (Quantity::from("3.000").raw(), Quantity::from("2.000").raw())
    );
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(engine.instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("3.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    let cache = engine.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cached.avg_px(), Some(dec!(99)));
}

#[rstest]
#[case::buy(OrderSide::Buy, "99.00", "100.00", "99.00", "101.00")]
#[case::sell(OrderSide::Sell, "102.00", "101.00", "100.00", "102.00")]
fn test_l1_quote_return_to_order_price_preserves_queue(
    #[case] side: OrderSide,
    #[case] initial: &str,
    #[case] improved: &str,
    #[case] bid: &str,
    #[case] ask: &str,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);

    let quote = |price: &str, size: &str, ts| {
        QuoteTick::new(
            instrument.id(),
            Price::from(if side == OrderSide::Buy { price } else { bid }),
            Price::from(if side == OrderSide::Sell { price } else { ask }),
            Quantity::from(size),
            Quantity::from(size),
            ts,
            ts,
        )
    };

    engine.process_quote_tick(&quote(initial, "5.000", 1.into()));
    let id = ClientOrderId::from("QUEUE-RETURN");
    let price = Price::from(initial);
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(side)
        .price(price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    engine.process_quote_tick(&quote(improved, "7.000", 2.into()));
    engine.process_quote_tick(&quote(initial, "6.000", 3.into()));
    assert_eq!(
        engine.queue_ahead_total[&id],
        (price.raw(), Quantity::from("5.000").raw())
    );
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        price,
        Quantity::from("3.000"),
        if side == OrderSide::Buy {
            AggressorSide::Sell
        } else {
            AggressorSide::Buy
        },
        TradeId::from("RETURN-TRADE"),
        4.into(),
        4.into(),
    ));

    assert_eq!(
        engine.queue_ahead_total[&id],
        (price.raw(), Quantity::from("2.000").raw())
    );
    assert_eq!(
        cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );
    assert_eq!(
        cache.borrow().order(&id).unwrap().filled_qty(),
        Quantity::from("0.000")
    );
}

#[rstest]
fn test_l1_pending_sell_resolves_from_first_quote_after_trade() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("INITIAL"),
        1.into(),
        1.into(),
    ));
    let id = ClientOrderId::from("PENDING-SELL");
    let price = Price::from("101.00");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(OrderSide::Sell)
        .price(price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(engine.queue_pending.get(&id), Some(&price.raw()));
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("100.00"),
        price,
        Quantity::from("7.000"),
        Quantity::from("10.000"),
        2.into(),
        2.into(),
    ));
    assert!(!engine.queue_pending.contains_key(&id));
    assert_eq!(
        engine.queue_ahead_total[&id],
        (price.raw(), Quantity::from("10.000").raw())
    );
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        price,
        Quantity::from("12.000"),
        AggressorSide::Buy,
        TradeId::from("RESOLVED"),
        3.into(),
        3.into(),
    ));
    let cache = cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.status(), OrderStatus::Filled);
    assert_eq!(cached.filled_qty(), Quantity::from("2.000"));
    assert_eq!(cached.avg_px(), Some(dec!(101)));
}

#[rstest]
#[case::buy(
    OrderSide::Buy,
    "99.00",
    "98.00",
    AggressorSide::Buy,
    AggressorSide::Sell
)]
#[case::sell(
    OrderSide::Sell,
    "102.00",
    "103.00",
    AggressorSide::Sell,
    AggressorSide::Buy
)]
fn test_l1_pending_queue_resolves_on_opposite_aggressor_trade_through(
    #[case] side: OrderSide,
    #[case] limit: &str,
    #[case] crossed: &str,
    #[case] opposite: AggressorSide,
    #[case] matching: AggressorSide,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        1.into(),
        1.into(),
    ));
    let id = ClientOrderId::from("PENDING-THROUGH");
    let price = Price::from(limit);
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(side)
        .price(price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(engine.queue_pending.get(&id), Some(&price.raw()));
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        Price::from(crossed),
        Quantity::from("2.000"),
        opposite,
        TradeId::from("THROUGH"),
        2.into(),
        2.into(),
    ));
    assert!(!engine.queue_pending.contains_key(&id));
    assert_eq!(engine.queue_ahead_total[&id], (price.raw(), 0));
    assert_eq!(
        cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        price,
        Quantity::from("2.000"),
        matching,
        TradeId::from("AT-LIMIT"),
        3.into(),
        3.into(),
    ));
    assert_eq!(
        cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(
        cache.borrow().order(&id).unwrap().filled_qty(),
        Quantity::from("2.000")
    );
}

#[rstest]
fn test_market_to_limit_stops_after_first_book_level(#[values(false, true)] deferred: bool) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, true, false);
    engine.book.clear(1, 1.into());
    for (price, id) in [("100.00", 1), ("101.00", 2), ("102.00", 3)] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from(price),
                    Quantity::from("1.000"),
                    id,
                ),
                RecordFlag::F_LAST as u8,
                id,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    if deferred {
        let saved = events.clone();
        engine.set_event_handler(Rc::new(move |event| saved.borrow_mut().push(event)));
    }

    let id = ClientOrderId::from("MTL-FIRST-LEVEL");
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("3.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    let saved = events.borrow();
    assert_eq!(saved.len(), 3);

    let OrderEventAny::Updated(update) = &saved[0] else {
        panic!("Expected Updated")
    };

    let OrderEventAny::Filled(fill) = &saved[1] else {
        panic!("Expected Filled")
    };

    let OrderEventAny::Accepted(accepted) = &saved[2] else {
        panic!("Expected Accepted")
    };

    assert_eq!(update.client_order_id, id);
    assert_eq!(update.price, Some(Price::from("100.00")));
    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.last_px, Price::from("100.00"));
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert_eq!(accepted.client_order_id, id);
    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.price(), Some(Price::from("100.00")));
    assert_eq!(cached.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cached.leaves_qty(), Quantity::from("2.000"));
    assert_eq!(engine.core.get_orders().len(), 1);
    assert_eq!(
        engine.core.get_order(id).unwrap().limit_price,
        Some(Price::from("100.00"))
    );
    drop(cached);
    drop(cache);
    drop(saved);
    events.borrow_mut().clear();
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            engine.instrument.id(),
            BookAction::Update,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("3.000"),
                1,
            ),
            RecordFlag::F_LAST as u8,
            4,
            2.into(),
            2.into(),
        ))
        .unwrap();

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    assert_eq!(
        events
            .borrow()
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        vec![OrderEventType::Filled]
    );
    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.status(), OrderStatus::Filled);
    assert_eq!(cached.filled_qty(), Quantity::from("3.000"));
    assert!(!engine.core.order_exists(id));
}

#[rstest]
#[case::pause_open(MarketStatusAction::Pause, MarketStatus::Open, MarketStatus::Paused)]
#[case::pause_closed(MarketStatusAction::Pause, MarketStatus::Closed, MarketStatus::Closed)]
#[case::pause_paused(MarketStatusAction::Pause, MarketStatus::Paused, MarketStatus::Paused)]
#[case::pause_suspended(
    MarketStatusAction::Pause,
    MarketStatus::Suspended,
    MarketStatus::Suspended
)]
#[case::suspend_open(
    MarketStatusAction::Suspend,
    MarketStatus::Open,
    MarketStatus::Suspended
)]
#[case::suspend_closed(
    MarketStatusAction::Suspend,
    MarketStatus::Closed,
    MarketStatus::Closed
)]
#[case::suspend_paused(
    MarketStatusAction::Suspend,
    MarketStatus::Paused,
    MarketStatus::Paused
)]
#[case::suspend_suspended(
    MarketStatusAction::Suspend,
    MarketStatus::Suspended,
    MarketStatus::Suspended
)]
fn test_pause_and_suspend_only_change_open_market(
    #[case] action: MarketStatusAction,
    #[case] initial: MarketStatus,
    #[case] expected: MarketStatus,
) {
    let (mut engine, _) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.market_status = initial;
    engine.process_status(action);
    assert_eq!(engine.market_status, expected);
}

#[rstest]
#[case::long_bid(OrderSide::Buy, Some(OrderSide::Buy), true)]
#[case::long_ask(OrderSide::Buy, Some(OrderSide::Sell), false)]
#[case::short_ask(OrderSide::Sell, Some(OrderSide::Sell), true)]
#[case::short_bid(OrderSide::Sell, Some(OrderSide::Buy), false)]
#[case::long_empty(OrderSide::Buy, None, false)]
#[case::short_empty(OrderSide::Sell, None, false)]
fn test_liquidation_requires_price_on_closing_side(
    #[case] opening: OrderSide,
    #[case] quoted: Option<OrderSide>,
    #[case] fills: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.book.clear(1, 1.into());
    engine.core.bid = None;
    engine.core.ask = None;
    if let Some(side) = quoted {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side, Price::from("123.45"), Quantity::from("5.000"), 1),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    let (_, fill) = pending_position_fill(
        &engine.instrument,
        PositionId::from("LIQ-POS"),
        "LIQ-OPEN",
        opening,
        "2.000",
    );
    let account_id = fill.account_id;
    let position = Position::new(&engine.instrument, fill);
    engine
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let orders_before = engine
        .cache
        .borrow()
        .orders_total_count(None, None, None, None, None);
    engine.liquidate_open_positions(2.into(), false, engine.instrument.settlement_currency());
    let saved = events.borrow();

    if !fills {
        assert!(saved.is_empty());
        assert_eq!(
            engine
                .cache
                .borrow()
                .orders_total_count(None, None, None, None, None),
            orders_before
        );
        assert_eq!(
            engine
                .cache
                .borrow()
                .position(&position.id)
                .unwrap()
                .quantity,
            Quantity::from("2.000")
        );
        return;
    }

    assert_eq!(saved.len(), 3);
    assert!(matches!(&saved[0], OrderEventAny::Submitted(_)));
    assert!(matches!(&saved[1], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &saved[2] else {
        panic!("Expected liquidation fill")
    };

    assert_eq!(fill.instrument_id, engine.instrument.id());
    assert_eq!(fill.account_id, account_id);
    assert_eq!(
        engine.cache.borrow().position_id(&fill.client_order_id),
        Some(&position.id)
    );
    assert_eq!(fill.position_id, None);
    assert_eq!(
        fill.order_side,
        if opening == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    );
    assert_eq!(fill.order_type, OrderType::Market);
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.last_px, Price::from("123.45"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert_eq!(
        fill.commission,
        Some(Money::zero(engine.instrument.quote_currency()))
    );
}

#[rstest]
fn test_liquidation_other_settlement_currency_preserves_orders_and_positions() {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    let (_, fill) = pending_position_fill(
        &engine.instrument,
        PositionId::from("HEALTHY-POS"),
        "HEALTHY-OPEN",
        OrderSide::Buy,
        "2.000",
    );
    let position = Position::new(&engine.instrument, fill);
    engine
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("HEALTHY-ORDER"))
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    events.borrow_mut().clear();
    engine.liquidate_open_positions(2.into(), true, Currency::JPY());
    assert!(events.borrow().is_empty());
    let cache = engine.cache.borrow();
    assert_eq!(
        cache.position(&position.id).unwrap().quantity,
        Quantity::from("2.000")
    );
    assert_eq!(
        cache.order(&order.client_order_id()).unwrap().status(),
        OrderStatus::Accepted
    );
    assert!(engine.core.order_exists(order.client_order_id()));
}

#[rstest]
fn test_l1_opposite_orders_share_cached_trade_liquidity() {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, true, false);
    engine.config.trade_execution = true;
    engine.process_trade_tick(&TradeTick::new(
        engine.instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("SHARED"),
        2.into(),
        2.into(),
    ));
    let ids = [
        ClientOrderId::from("SHARED-BUY"),
        ClientOrderId::from("SHARED-SELL"),
    ];

    for (id, side) in [(ids[0], OrderSide::Buy), (ids[1], OrderSide::Sell)] {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(engine.instrument.id())
            .client_order_id(id)
            .side(side)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("6.000"))
            .submit(true)
            .build();
        process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    }

    let saved = events.borrow();

    let fills: Vec<_> = saved
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        fills,
        vec![
            (ids[0], Price::from("100.00"), Quantity::from("6.000")),
            (ids[1], Price::from("100.00"), Quantity::from("4.000")),
        ]
    );
    assert_eq!(engine.trade_consumption, Quantity::from("10.000").raw());
    let cache = engine.cache.borrow();
    assert_eq!(cache.order(&ids[0]).unwrap().status(), OrderStatus::Filled);
    assert_eq!(
        cache.order(&ids[1]).unwrap().status(),
        OrderStatus::PartiallyFilled
    );
    assert_eq!(
        cache.order(&ids[1]).unwrap().leaves_qty(),
        Quantity::from("2.000")
    );
}

#[rstest]
#[case::zero("0.000", 0)]
#[case::one("0.001", 1)]
#[case::two("0.002", 2)]
#[case::three("0.003", 3)]
#[case::four("0.004", 4)]
fn test_flat_bar_processes_allocated_volume(
    #[values("LAST", "MID")] price_type: &str,
    #[case] volume: &str,
    #[case] increments: usize,
    #[values(false, true)] liquidity_consumption: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, liquidity_consumption, false);
    engine.book.reset();
    engine.core.reset();
    engine.config.bar_execution = true;
    let id = ClientOrderId::from("FLAT-BAR");
    let price = Price::from("100.00");
    let order_qty = Quantity::from("0.005");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .price(price)
        .quantity(order_qty)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    events.borrow_mut().clear();
    let bar_type =
        BarType::from(format!("ETHUSDT-PERP.BINANCE-1-MINUTE-{price_type}-EXTERNAL").as_str());
    let volume = Quantity::from(volume);
    engine.process_bar(&Bar::new(
        bar_type,
        price,
        price,
        price,
        price,
        volume,
        2.into(),
        2.into(),
    ));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    let market_price = (increments > 0).then_some(price);
    let market_size = (increments > 0).then_some(Quantity::from("0.001"));
    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();

    let fill_count = if liquidity_consumption {
        increments.min(1)
    } else {
        increments
    };

    let filled_qty = if fill_count == 0 {
        Quantity::zero(3)
    } else if liquidity_consumption {
        Quantity::from("0.001")
    } else {
        volume
    };

    assert_eq!(engine.instrument.size_increment(), Quantity::from("0.001"));
    assert_eq!(engine.core.last, Some(price));
    assert_eq!(engine.core.bid, market_price);
    assert_eq!(engine.core.ask, market_price);
    assert_eq!(engine.book.best_bid_price(), market_price);
    assert_eq!(engine.book.best_ask_price(), market_price);
    assert_eq!(engine.book.best_bid_size(), market_size);
    assert_eq!(engine.book.best_ask_size(), market_size);
    assert_eq!(
        fills,
        vec![(id, price, Quantity::from("0.001")); fill_count]
    );
    assert_eq!(cached.filled_qty(), filled_qty);
    assert_eq!(cached.leaves_qty(), order_qty - filled_qty);
    assert_eq!(
        cached.status(),
        if increments == 0 {
            OrderStatus::Accepted
        } else {
            OrderStatus::PartiallyFilled
        }
    );
    assert!(engine.fill_at_market);
}

#[rstest]
fn test_later_flat_bar_processes_repeated_price_legs(#[values("LAST", "MID")] price_type: &str) {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, false, false);
    engine.config.bar_execution = true;
    let price = Price::from("100.00");
    let bar_type =
        BarType::from(format!("ETHUSDT-PERP.BINANCE-1-MINUTE-{price_type}-EXTERNAL").as_str());

    let mut bar = Bar::new(
        bar_type,
        price,
        price,
        price,
        price,
        Quantity::from("0.004"),
        2.into(),
        2.into(),
    );
    engine.process_bar(&bar);
    let id = ClientOrderId::from("LATER-FLAT-BAR");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .price(price)
        .quantity(Quantity::from("0.006"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().filled_qty(),
        Quantity::from("0.001")
    );
    events.borrow_mut().clear();
    bar.ts_event = 3.into();
    bar.ts_init = 3.into();
    engine.process_bar(&bar);

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();

    assert_eq!(fills, vec![(id, price, Quantity::from("0.001")); 4]);
    assert_eq!(cached.status(), OrderStatus::PartiallyFilled);
    assert_eq!(cached.filled_qty(), Quantity::from("0.005"));
    assert_eq!(cached.leaves_qty(), Quantity::from("0.001"));
    assert_eq!(engine.book.best_bid_price(), Some(price));
    assert_eq!(engine.book.best_ask_price(), Some(price));
    assert_eq!(engine.book.best_bid_size(), Some(Quantity::from("0.001")));
    assert_eq!(engine.book.best_ask_size(), Some(Quantity::from("0.001")));
    assert!(engine.fill_at_market);
}

#[rstest]
#[case::one("0.001")]
#[case::two("0.002")]
fn test_flat_bar_stop_fills_at_open(
    #[values("LAST", "MID")] price_type: &str,
    #[values(false, true)] first_bar: bool,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[case] volume: &str,
) {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, false, false);
    engine.book.reset();
    engine.core.reset();
    engine.config.bar_execution = true;
    let bar_type =
        BarType::from(format!("ETHUSDT-PERP.BINANCE-1-MINUTE-{price_type}-EXTERNAL").as_str());

    if !first_bar {
        let price = Price::from("100.00");
        engine.process_bar(&Bar::new(
            bar_type,
            price,
            price,
            price,
            price,
            Quantity::from("0.004"),
            2.into(),
            2.into(),
        ));
    }

    let (trigger_price, open_price) = match side {
        OrderSide::Buy => (Price::from("105.00"), Price::from("110.00")),
        OrderSide::Sell => (Price::from("95.00"), Price::from("90.00")),
    };

    let id = ClientOrderId::from("FLAT-STOP");
    let order_qty = Quantity::from("0.001");
    let mut order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(side)
        .trigger_price(trigger_price)
        .quantity(order_qty)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    events.borrow_mut().clear();
    engine.process_bar(&Bar::new(
        bar_type,
        open_price,
        open_price,
        open_price,
        open_price,
        Quantity::from(volume),
        3.into(),
        3.into(),
    ));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((
                fill.client_order_id,
                fill.order_side,
                fill.last_px,
                fill.last_qty,
                fill.liquidity_side,
            )),
            _ => None,
        })
        .collect();

    let cache = engine.cache.borrow();
    let cached = cache.order(&id).unwrap();

    assert_eq!(
        fills,
        vec![(id, side, open_price, order_qty, LiquiditySide::Taker)]
    );
    assert_eq!(cached.status(), OrderStatus::Filled);
    assert_eq!(cached.filled_qty(), order_qty);
    assert_eq!(cached.leaves_qty(), Quantity::zero(3));
    assert!(engine.fill_at_market);
}

#[rstest]
fn test_bar_gap_stop_fills_at_open() {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, false, false);
    engine.config.bar_execution = true;
    let bar_type = BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL");
    engine.process_bar(&Bar::new(
        bar_type,
        Price::from("100.00"),
        Price::from("100.00"),
        Price::from("100.00"),
        Price::from("100.00"),
        Quantity::from("100.000"),
        2.into(),
        2.into(),
    ));
    let id = ClientOrderId::from("GAP-STOP");
    let mut order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .trigger_price(Price::from("105.00"))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );
    events.borrow_mut().clear();
    engine.process_bar(&Bar::new(
        bar_type,
        Price::from("110.00"),
        Price::from("112.00"),
        Price::from("109.00"),
        Price::from("111.00"),
        Quantity::from("100.000"),
        3.into(),
        3.into(),
    ));
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected gap fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.last_px, Price::from("110.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
}

#[rstest]
fn test_quote_bar_close_restores_market_fill_and_clears_pair() {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, false, false);
    engine.config.bar_execution = true;
    engine.config.reject_stop_orders = false;

    for (bar_type, price) in [
        ("ETHUSDT-PERP.BINANCE-1-MINUTE-BID-EXTERNAL", "100.00"),
        ("ETHUSDT-PERP.BINANCE-1-MINUTE-ASK-EXTERNAL", "101.00"),
    ] {
        let price = Price::from(price);
        engine.process_bar(&Bar::new(
            BarType::from(bar_type),
            price,
            price,
            price,
            price,
            Quantity::from("100.000"),
            2.into(),
            2.into(),
        ));
    }

    let id = ClientOrderId::from("AFTER-QUOTE-BAR");
    let mut order = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .trigger_price(Price::from("100.00"))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected market fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.last_px, Price::from("101.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert!(engine.fill_at_market);
    assert_eq!(engine.last_bar_bid, None);
    assert_eq!(engine.last_bar_ask, None);
}

#[rstest]
fn test_trailing_limit_without_trigger_stays_inactive_when_market_arrives() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L1_MBP);
    let id = ClientOrderId::from("INACTIVE-TRAILING");
    let mut order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(OrderSide::Sell)
        .price(Price::from("99.00"))
        .activation_price(Price::from("110.00"))
        .trailing_offset(dec!(1))
        .limit_offset(dec!(1))
        .trailing_offset_type(TrailingOffsetType::Price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        1.into(),
        1.into(),
    ));
    let cache = cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.status(), OrderStatus::Accepted);
    assert_eq!(cached.trigger_price(), None);
    assert_eq!(cached.is_triggered(), Some(false));
    assert_eq!(cached.filled_qty(), Quantity::from("0.000"));
    assert_eq!(cached.leaves_qty(), Quantity::from("2.000"));
    let resting = engine.core.get_order(id).unwrap();
    assert!(!resting.is_activated);
    assert_eq!(resting.limit_price, None);
    assert_eq!(resting.trigger_price, None);
}

#[rstest]
#[case::market(OrderType::TrailingStopMarket)]
#[case::limit(OrderType::TrailingStopLimit)]
fn test_trailing_last_or_bid_ask_activation_uses_last_before_quote(#[case] order_type: OrderType) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (mut engine, cache) = get_queue_engine(instrument.clone(), BookType::L2_MBP);
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("LAST-ONLY"),
        1.into(),
        1.into(),
    ));
    let id = ClientOrderId::from("LAST-OR-QUOTE-TRAILING");
    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .trigger_type(TriggerType::LastOrBidAsk)
        .trailing_offset(dec!(2))
        .limit_offset(dec!(1))
        .trailing_offset_type(TrailingOffsetType::Price)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        cache.borrow().order(&id).unwrap().activation_price(),
        Some(Price::from("100.00"))
    );
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("MATERIALIZE"),
        2.into(),
        2.into(),
    ));
    let cache_ref = cache.borrow();
    let cached = cache_ref.order(&id).unwrap();
    assert_eq!(cached.status(), OrderStatus::Accepted);
    assert_eq!(cached.activation_price(), Some(Price::from("100.00")));
    assert_eq!(cached.trigger_price(), Some(Price::from("102.00")));
    assert!(engine.core.get_order(id).unwrap().is_activated);
    assert_eq!(
        cached.price(),
        if order_type == OrderType::TrailingStopLimit {
            Some(Price::from("101.00"))
        } else {
            None
        }
    );
    drop(cached);
    drop(cache_ref);
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            instrument.id(),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("10.000"),
                1,
            ),
            RecordFlag::F_LAST as u8,
            1,
            3.into(),
            3.into(),
        ))
        .unwrap();
    engine.process_trade_tick(&TradeTick::new(
        instrument.id(),
        Price::from("103.00"),
        Quantity::from("10.000"),
        AggressorSide::Buy,
        TradeId::from("TRAILING-TRIGGER"),
        4.into(),
        4.into(),
    ));
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            instrument.id(),
            BookAction::Update,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("100.00"),
                Quantity::from("10.000"),
                1,
            ),
            RecordFlag::F_LAST as u8,
            2,
            5.into(),
            5.into(),
        ))
        .unwrap();
    let cache = cache.borrow();
    let cached = cache.order(&id).unwrap();
    assert_eq!(cached.status(), OrderStatus::Filled);
    assert_eq!(cached.filled_qty(), Quantity::from("2.000"));
    assert_eq!(
        cached.avg_px(),
        Some(if order_type == OrderType::TrailingStopLimit {
            dec!(101)
        } else {
            dec!(100)
        })
    );
}

#[rstest]
#[case::buy(OrderSide::Buy, "105.00", "103.00", "99.00", "100.00")]
#[case::sell(OrderSide::Sell, "95.00", "97.00", "100.00", "101.00")]
fn test_last_triggered_stop_limit_inside_book_fills_as_maker(
    #[case] side: OrderSide,
    #[case] trigger: &str,
    #[case] limit: &str,
    #[case] bid: &str,
    #[case] ask: &str,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.trade_execution = false;
    engine.fee_model =
        FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.001), dec!(0.002))).into();
    engine.book.clear(1, 1.into());
    for (side, price, id) in [(OrderSide::Buy, bid, 1), (OrderSide::Sell, ask, 2)] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side, Price::from(price), Quantity::from("10.000"), id),
                RecordFlag::F_LAST as u8,
                id,
                2.into(),
                2.into(),
            ))
            .unwrap();
    }

    let id = ClientOrderId::from("LAST-MAKER");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(side)
        .trigger_type(TriggerType::LastPrice)
        .trigger_price(Price::from(trigger))
        .price(Price::from(limit))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );
    events.borrow_mut().clear();
    engine.process_trade_tick(&TradeTick::new(
        engine.instrument.id(),
        Price::from(trigger),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("TRIGGER"),
        3.into(),
        3.into(),
    ));
    let saved = events.borrow();
    assert_eq!(saved.len(), 2);
    assert!(matches!(&saved[0], OrderEventAny::Triggered(event) if event.client_order_id == id));

    let OrderEventAny::Filled(fill) = &saved[1] else {
        panic!("Expected maker fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.order_side, side);
    assert_eq!(fill.last_px, Price::from(limit));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
    let commission = Price::from(limit).as_decimal() * dec!(2) * dec!(0.001);
    assert_eq!(
        fill.commission,
        Some(Money::from_decimal(commission, engine.instrument.quote_currency()).unwrap())
    );
}

#[rstest]
fn test_equal_trigger_and_limit_keeps_taker_price_after_resting() {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.trade_execution = false;
    engine.fee_model =
        FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.001), dec!(0.002))).into();
    engine.book.clear(1, 1.into());
    for (side, price, id) in [(OrderSide::Buy, "99.00", 1), (OrderSide::Sell, "101.00", 2)] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side, Price::from(price), Quantity::from("10.000"), id),
                RecordFlag::F_LAST as u8,
                id,
                2.into(),
                2.into(),
            ))
            .unwrap();
    }

    let id = ClientOrderId::from("EQUAL-TRIGGER-LIMIT");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Buy)
        .trigger_type(TriggerType::LastPrice)
        .trigger_price(Price::from("100.00"))
        .price(Price::from("100.00"))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    engine.process_trade_tick(&TradeTick::new(
        engine.instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("EQUAL-TRIGGER"),
        3.into(),
        3.into(),
    ));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Triggered
    );
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().liquidity_side(),
        Some(LiquiditySide::Taker)
    );
    events.borrow_mut().clear();
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            engine.instrument.id(),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("99.00"),
                Quantity::from("10.000"),
                3,
            ),
            RecordFlag::F_LAST as u8,
            3,
            4.into(),
            4.into(),
        ))
        .unwrap();
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected taker fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.last_px, Price::from("99.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    assert_eq!(
        fill.commission,
        Some(Money::from_decimal(dec!(0.396), engine.instrument.quote_currency()).unwrap())
    );
}

#[rstest]
#[case::buy(OrderSide::Buy, "99.00", "100.00")]
#[case::sell(OrderSide::Sell, "100.00", "101.00")]
fn test_quote_touch_fills_inside_limit_without_own_side_probability_gate(
    #[case] side: OrderSide,
    #[case] bid: &str,
    #[case] ask: &str,
) {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, false, false);
    engine.set_fill_model(
        FillModelAny::Default(DefaultFillModel::new(0.0, 0.0, Some(42)).unwrap()).into(),
    );
    engine.process_quote_tick(&QuoteTick::new(
        engine.instrument.id(),
        Price::from("99.00"),
        Price::from("101.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        2.into(),
        2.into(),
    ));
    let id = ClientOrderId::from("QUOTE-TOUCH");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(side)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Accepted
    );
    events.borrow_mut().clear();
    engine.process_quote_tick(&QuoteTick::new(
        engine.instrument.id(),
        Price::from(bid),
        Price::from(ask),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        3.into(),
        3.into(),
    ));
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected quote fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.order_side, side);
    assert_eq!(fill.last_px, Price::from("100.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(engine.cached_filled_qty_len(), 0);
}

#[rstest]
fn test_oco_fill_does_not_cancel_already_closed_sibling() {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.support_contingent_orders = true;
    let ids = [
        ClientOrderId::from("OCO-FILLED"),
        ClientOrderId::from("OCO-CLOSED"),
    ];

    let mut orders: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(engine.instrument.id())
                .client_order_id(*id)
                .side(OrderSide::Sell)
                .price(Price::from(if i == 0 { "1500.00" } else { "1502.00" }))
                .quantity(Quantity::from("2.000"))
                .contingency_type(ContingencyType::Oco)
                .linked_order_ids(vec![ids[1 - i]])
                .submit(true)
                .build()
        })
        .collect();

    for order in &orders {
        engine
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    for order in &mut orders {
        engine.process_order(order, AccountId::from("SIM-001"));
    }

    let sibling = engine.cache.borrow().order_owned(&ids[1]).unwrap();
    engine
        .cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::canceled(
            &sibling,
            AccountId::from("SIM-001"),
            sibling.venue_order_id(),
        ))
        .unwrap();
    events.borrow_mut().clear();
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            engine.instrument.id(),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1501.00"),
                Quantity::from("2.000"),
                2,
            ),
            RecordFlag::F_LAST as u8,
            2,
            2.into(),
            2.into(),
        ))
        .unwrap();
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected OCO fill")
    };

    assert_eq!(fill.client_order_id, ids[0]);
    assert_eq!(fill.last_px, Price::from("1500.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(
        engine.cache.borrow().order(&ids[1]).unwrap().status(),
        OrderStatus::Canceled
    );
}

#[rstest]
fn test_parent_modify_does_not_resize_canceled_oto_child() {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.support_contingent_orders = true;
    let parent_id = ClientOrderId::from("OTO-PARENT");
    let child_id = ClientOrderId::from("OTO-CANCELED-CHILD");
    let mut parent = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(parent_id)
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("10.000"))
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .submit(true)
        .build();
    let mut child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .price(Price::from("1600.00"))
        .quantity(Quantity::from("10.000"))
        .parent_order_id(parent_id)
        .submit(true)
        .build();

    for order in [&parent, &child] {
        engine
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    let account_id = AccountId::from("SIM-001");
    engine.process_order(&mut parent, account_id);
    engine.process_order(&mut child, account_id);
    engine.cancel_order(&child, None);
    assert_eq!(
        engine.cache.borrow().order(&child_id).unwrap().status(),
        OrderStatus::Canceled
    );
    assert!(
        !engine
            .orders
            .closed_oto_order_ids
            .borrow()
            .contains(&child_id)
    );
    events.borrow_mut().clear();
    engine.process_modify(
        &ModifyOrder::new(
            parent.trader_id(),
            None,
            parent.strategy_id(),
            parent.instrument_id(),
            parent_id,
            None,
            Some(Quantity::from("5.000")),
            None,
            None,
            UUID4::new(),
            2.into(),
            None,
            None,
        ),
        account_id,
    );
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Updated(update) = &saved[0] else {
        panic!("Expected parent update")
    };

    assert_eq!(update.client_order_id, parent_id);
    assert_eq!(update.quantity, Quantity::from("5.000"));
    let cache = engine.cache.borrow();
    let cached_child = cache.order(&child_id).unwrap();
    assert_eq!(cached_child.status(), OrderStatus::Canceled);
    assert_eq!(cached_child.quantity(), Quantity::from("10.000"));
    assert_eq!(cached_child.filled_qty(), Quantity::from("0.000"));
}

#[rstest]
fn test_triggered_sell_maker_modified_below_trigger_keeps_trigger_fill_price() {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.trade_execution = false;
    engine.fee_model =
        FeeModelAny::MakerTaker(MakerTakerFeeModel::new(dec!(0.001), dec!(0.002))).into();
    engine.book.clear(1, 1.into());
    for (side, price, id) in [(OrderSide::Buy, "99.00", 1), (OrderSide::Sell, "101.00", 2)] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side, Price::from(price), Quantity::from("10.000"), id),
                RecordFlag::F_LAST as u8,
                id,
                2.into(),
                2.into(),
            ))
            .unwrap();
    }

    let id = ClientOrderId::from("SELL-MAKER-MODIFIED");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(OrderSide::Sell)
        .trigger_type(TriggerType::LastPrice)
        .trigger_price(Price::from("100.00"))
        .price(Price::from("102.00"))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    let account_id = AccountId::from("SIM-001");
    process_cached_order(&mut engine, &mut order, account_id);
    engine.process_trade_tick(&TradeTick::new(
        engine.instrument.id(),
        Price::from("100.00"),
        Quantity::from("10.000"),
        AggressorSide::Sell,
        TradeId::from("SELL-TRIGGER"),
        3.into(),
        3.into(),
    ));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Triggered
    );
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().liquidity_side(),
        Some(LiquiditySide::Maker)
    );
    engine.process_modify(
        &ModifyOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            id,
            None,
            None,
            Some(Price::from("99.50")),
            None,
            UUID4::new(),
            4.into(),
            None,
            None,
        ),
        account_id,
    );
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().price(),
        Some(Price::from("99.50"))
    );
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().filled_qty(),
        Quantity::from("0.000")
    );
    events.borrow_mut().clear();
    engine
        .process_order_book_delta(&OrderBookDelta::new(
            engine.instrument.id(),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("100.50"),
                Quantity::from("10.000"),
                3,
            ),
            RecordFlag::F_LAST as u8,
            3,
            5.into(),
            5.into(),
        ))
        .unwrap();
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected maker fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.last_px, Price::from("100.00"));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
    assert_eq!(
        fill.commission,
        Some(Money::from_decimal(dec!(0.2), engine.instrument.quote_currency()).unwrap())
    );
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
}

#[rstest]
#[case::buy(OrderSide::Buy, "1501.00")]
#[case::sell(OrderSide::Sell, "1499.00")]
fn test_market_fill_immediately_purges_closed_order_fill_counter(
    #[case] side: OrderSide,
    #[case] price: &str,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    let id = ClientOrderId::from("MARKET-COUNTER");
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(engine.instrument.id())
        .client_order_id(id)
        .side(side)
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    let saved = events.borrow();
    assert_eq!(saved.len(), 1);

    let OrderEventAny::Filled(fill) = &saved[0] else {
        panic!("Expected market fill")
    };

    assert_eq!(fill.client_order_id, id);
    assert_eq!(fill.order_side, side);
    assert_eq!(fill.last_px, Price::from(price));
    assert_eq!(fill.last_qty, Quantity::from("2.000"));
    assert_eq!(
        engine.cache.borrow().order(&id).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(engine.cached_filled_qty_len(), 0);
}

#[rstest]
#[case::sized_gtc("2.000", "10.000", TimeInForce::Gtc)]
#[case::oversized_gtc("10.000", "10.000", TimeInForce::Gtc)]
#[case::sized_fok("2.000", "10.000", TimeInForce::Fok)]
#[case::oversized_fok("10.000", "10.000", TimeInForce::Fok)]
#[case::intermediate_depth_fok("10.000", "5.000", TimeInForce::Fok)]
fn test_reduce_only_consumes_executable_qty(
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(OrderType::Market, OrderType::Limit, OrderType::MarketToLimit)] order_type: OrderType,
    #[case] order_qty: &str,
    #[case] level_qty: &str,
    #[case] time_in_force: TimeInForce,
    #[values(false, true)] liquidity_consumption: bool,
    #[values(false, true)] synthetic: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, liquidity_consumption, false);
    for (side, price) in [(OrderSide::Buy, "1499.00"), (OrderSide::Sell, "1501.00")] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Update,
                BookOrder::new(side, Price::from(price), Quantity::from(level_qty), 0),
                RecordFlag::F_LAST as u8,
                2,
                2.into(),
                2.into(),
            ))
            .unwrap();
    }

    if synthetic {
        set_custom_fill_book(&mut engine, level_qty, false);
    }

    let (opening, fill) = pending_position_fill(
        &engine.instrument,
        PositionId::from("CONSUMPTION-POS"),
        "CONSUMPTION-OPEN",
        side.opposite(),
        "2.000",
    );
    let position = Position::new(&engine.instrument, fill.clone());
    engine
        .cache
        .borrow_mut()
        .add_order(opening, Some(position.id), None, false)
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(fill))
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let price = Price::from(if side == OrderSide::Buy {
        "1501.00"
    } else {
        "1499.00"
    });

    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("CONSUMPTION-CLOSE"))
        .side(side)
        .quantity(Quantity::from(order_qty))
        .time_in_force(time_in_force)
        .reduce_only(true)
        .submit(true);

    if order_type == OrderType::Limit {
        builder.price(price);
    }

    let mut order = builder.build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), Some(position.id), None, false)
        .unwrap();
    engine.process_order(&mut order, AccountId::from("SIM-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| {
            if let OrderEventAny::Filled(fill) = event {
                Some((fill.last_px, fill.last_qty))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(fills, vec![(price, Quantity::from("2.000"))]);

    let consumption = if side == OrderSide::Buy {
        &engine.ask_consumption
    } else {
        &engine.bid_consumption
    };

    assert_eq!(
        consumption.get(&price.raw()),
        (liquidity_consumption && !synthetic).then_some(&(
            Quantity::from(level_qty).raw(),
            Quantity::from("2.000").raw()
        ))
    );
    {
        let cache = engine.cache.borrow();
        let closed = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(closed.status(), OrderStatus::Filled);
        assert_eq!(closed.quantity(), Quantity::from("2.000"));
        assert_eq!(closed.filled_qty(), Quantity::from("2.000"));
        assert_eq!(closed.leaves_qty(), Quantity::from("0.000"));
    }

    events.borrow_mut().clear();
    let next_qty = Quantity::from(level_qty) - Quantity::from("2.000");
    let mut next = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("CONSUMPTION-NEXT"))
        .side(side)
        .price(price)
        .quantity(next_qty)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut next, AccountId::from("SIM-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| {
            if let OrderEventAny::Filled(fill) = event {
                Some((fill.last_px, fill.last_qty))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(fills, vec![(price, next_qty)]);
}

#[rstest]
#[case::buy(OrderSide::Buy, ["100.00", "101.00", "102.00"])]
#[case::sell(OrderSide::Sell, ["102.00", "101.00", "100.00"])]
fn test_market_consumes_only_executed_levels(
    #[case] side: OrderSide,
    #[case] prices: [&str; 3],
    #[values(OrderType::MarketToLimit, OrderType::Market)] order_type: OrderType,
    #[values(false, true)] consumed_top: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, true, false);
    engine.book.clear(1, 1.into());
    for (idx, price) in prices.iter().enumerate() {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(
                    side.opposite(),
                    Price::from(*price),
                    Quantity::from("1.000"),
                    idx as u64,
                ),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    if consumed_top {
        let consumption = if side == OrderSide::Buy {
            &mut engine.ask_consumption
        } else {
            &mut engine.bid_consumption
        };

        consumption.insert(
            Price::from(prices[0]).raw(),
            (Quantity::from("1.000").raw(), Quantity::from("1.000").raw()),
        );
    }

    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("LEVELS-FIRST"))
        .side(side)
        .quantity(Quantity::from("3.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));
    let start = usize::from(consumed_top);
    let end = if order_type == OrderType::MarketToLimit {
        start + 1
    } else {
        prices.len()
    };

    let expected: Vec<_> = prices[start..end]
        .iter()
        .map(|price| (Price::from(*price), Quantity::from("1.000")))
        .collect();

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| {
            if let OrderEventAny::Filled(fill) = event {
                Some((fill.last_px, fill.last_qty))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(fills, expected);

    let consumption = if side == OrderSide::Buy {
        &engine.ask_consumption
    } else {
        &engine.bid_consumption
    };

    let expected_consumption: Vec<_> = prices[..end]
        .iter()
        .map(|price| {
            (
                Price::from(*price).raw(),
                (Quantity::from("1.000").raw(), Quantity::from("1.000").raw()),
            )
        })
        .collect();

    assert_eq!(
        consumption.clone().into_iter().collect::<Vec<_>>(),
        expected_consumption
    );
    events.borrow_mut().clear();
    let mut next = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("LEVELS-NEXT"))
        .side(side)
        .price(Price::from(prices[2]))
        .quantity(Quantity::from("2.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut next, AccountId::from("SIM-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| {
            if let OrderEventAny::Filled(fill) = event {
                Some((fill.last_px, fill.last_qty))
            } else {
                None
            }
        })
        .collect();

    let expected: Vec<_> = prices[end..]
        .iter()
        .map(|price| (Price::from(*price), Quantity::from("1.000")))
        .collect();
    assert_eq!(fills, expected);
}

#[rstest]
fn test_reduce_only_trade_budget_uses_executable_qty(
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values("2.000", "10.000")] order_qty: &str,
) {
    let (mut engine, events) = custom_book_engine(BookType::L1_MBP, true, false);
    engine.config.trade_execution = true;
    let (opening, fill) = pending_position_fill(
        &engine.instrument,
        PositionId::from("TRADE-POS"),
        "TRADE-OPEN",
        side.opposite(),
        "2.000",
    );
    let position = Position::new(&engine.instrument, fill.clone());
    engine
        .cache
        .borrow_mut()
        .add_order(opening, Some(position.id), None, false)
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(fill))
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let price = Price::from("1500.00");
    let ids = [
        ClientOrderId::from("TRADE-CLOSE"),
        ClientOrderId::from("TRADE-NEXT"),
    ];
    let mut closing = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ids[0])
        .side(side)
        .price(price)
        .quantity(Quantity::from(order_qty))
        .reduce_only(true)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position.id), None, false)
        .unwrap();
    engine.process_order(&mut closing, AccountId::from("SIM-001"));
    let mut next = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ids[1])
        .side(side)
        .price(price)
        .quantity(Quantity::from("8.000"))
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut next, AccountId::from("SIM-001"));
    engine.process_trade_tick(&TradeTick::new(
        engine.instrument.id(),
        price,
        Quantity::from("10.000"),
        AggressorSide::NoAggressor,
        TradeId::from("TRADE-BUDGET"),
        2.into(),
        2.into(),
    ));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        fills,
        vec![
            (ids[0], price, Quantity::from("2.000")),
            (ids[1], price, Quantity::from("8.000"))
        ]
    );
    assert_eq!(engine.trade_consumption, Quantity::from("10.000").raw());
    assert!(engine.ask_consumption.is_empty());
    assert!(engine.bid_consumption.is_empty());

    let mut exhausted = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("TRADE-EXHAUSTED"))
        .side(side)
        .price(price)
        .quantity(Quantity::from("1.000"))
        .reduce_only(true)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(exhausted.clone(), Some(position.id), None, false)
        .unwrap();
    engine.accept_order(&mut exhausted);
    engine.fill_limit_order(exhausted.client_order_id());
    assert_eq!(
        engine
            .cache
            .borrow()
            .order(&exhausted.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Canceled
    );
    assert_eq!(engine.trade_consumption, Quantity::from("10.000").raw());
}

#[rstest]
#[case::buy(OrderSide::Buy, "1501.00")]
#[case::sell(OrderSide::Sell, "1499.00")]
fn test_queue_cap_consumes_only_fills_and_reverts_fok(
    #[case] side: OrderSide,
    #[case] limit: &str,
    #[values(TimeInForce::Gtc, TimeInForce::Fok)] time_in_force: TimeInForce,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, true, false);
    engine.config.queue_position = true;
    let price = Price::from(limit);
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("QUEUE-CONSUMPTION"))
        .side(side)
        .price(price)
        .quantity(Quantity::from("10.000"))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    order.set_liquidity_side(LiquiditySide::Maker);
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    engine
        .account_ids
        .insert(order.trader_id(), AccountId::from("SIM-001"));
    engine.accept_order(&mut order);
    engine.last_trade_size = Some(Quantity::from("10.000"));
    engine.core.last = Some(price);
    engine
        .queue_ahead_total
        .insert(order.client_order_id(), (price.raw(), 0));
    engine
        .queue_excess
        .insert(order.client_order_id(), Quantity::from("2.000").raw());
    engine.fill_limit_order(order.client_order_id());

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    let consumption = if side == OrderSide::Buy {
        &engine.ask_consumption
    } else {
        &engine.bid_consumption
    };

    let (expected_fills, expected_consumption, expected_trade, expected_status, next_qty) =
        match time_in_force {
            TimeInForce::Fok => (
                vec![],
                None,
                Quantity::from("0.000"),
                OrderStatus::Canceled,
                Quantity::from("10.000"),
            ),
            _ => (
                vec![(order.client_order_id(), price, Quantity::from("2.000"))],
                Some((
                    Quantity::from("10.000").raw(),
                    Quantity::from("2.000").raw(),
                )),
                Quantity::from("2.000"),
                OrderStatus::PartiallyFilled,
                Quantity::from("8.000"),
            ),
        };

    assert_eq!(fills, expected_fills);
    assert_eq!(consumption.get(&price.raw()).copied(), expected_consumption);
    assert_eq!(engine.trade_consumption, expected_trade.raw());
    assert_eq!(
        engine
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        expected_status
    );
    events.borrow_mut().clear();
    engine.config.queue_position = false;
    engine.last_trade_size = None;
    let mut next = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("QUEUE-CONSUMPTION-NEXT"))
        .side(side)
        .price(price)
        .quantity(next_qty)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut next, AccountId::from("SIM-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.client_order_id, fill.last_px, fill.last_qty))
            }
            _ => None,
        })
        .collect();

    assert_eq!(fills, vec![(next.client_order_id(), price, next_qty)]);
}

#[rstest]
#[case::buy(OrderSide::Buy, ["100.00", "101.00", "102.00"])]
#[case::sell(OrderSide::Sell, ["102.00", "101.00", "100.00"])]
fn test_market_to_limit_fok_reverts_unexecuted_levels(
    #[case] side: OrderSide,
    #[case] prices: [&str; 3],
    #[values(false, true)] liquidity_consumption: bool,
    #[values(false, true)] synthetic: bool,
    #[values(false, true)] deferred: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, liquidity_consumption, false);
    if deferred {
        let captured = events.clone();
        engine.set_event_handler(Rc::new(move |event| captured.borrow_mut().push(event)));
    }

    engine.book.clear(1, 1.into());
    for (idx, price) in prices.iter().enumerate() {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(
                    side.opposite(),
                    Price::from(*price),
                    Quantity::from("1.000"),
                    idx as u64,
                ),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    if synthetic {
        engine.set_fill_model(FillModelHandle::new(TestBookFillModel {
            book: engine.book.clone(),
        }));
    }

    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("MTL-FOK"))
        .side(side)
        .quantity(Quantity::from("3.000"))
        .time_in_force(TimeInForce::Fok)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    assert_eq!(
        events
            .borrow()
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        vec![OrderEventType::Canceled]
    );
    assert!(engine.ask_consumption.is_empty());
    assert!(engine.bid_consumption.is_empty());
    assert_eq!(engine.trade_consumption, 0);
    assert!(!engine.core.order_exists(order.client_order_id()));

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    assert_eq!(
        engine
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Canceled
    );
    events.borrow_mut().clear();
    let mut next = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("MTL-FOK-NEXT"))
        .side(side)
        .price(Price::from(prices[2]))
        .quantity(Quantity::from("3.000"))
        .time_in_force(TimeInForce::Fok)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut next, AccountId::from("SIM-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_px, fill.last_qty)),
            _ => None,
        })
        .collect();

    let expected: Vec<_> = prices
        .iter()
        .map(|price| (Price::from(*price), Quantity::from("1.000")))
        .collect();
    assert_eq!(fills, expected);
}

#[rstest]
#[case::l2_real(BookType::L2_MBP, false, false, false, OrderSide::Buy)]
#[case::l2_consumption(BookType::L2_MBP, true, false, true, OrderSide::Sell)]
#[case::l2_synthetic(BookType::L2_MBP, false, true, true, OrderSide::Sell)]
#[case::l2_synthetic_consumption(BookType::L2_MBP, true, true, false, OrderSide::Buy)]
#[case::l3_real(BookType::L3_MBO, false, false, true, OrderSide::Sell)]
#[case::l3_consumption(BookType::L3_MBO, true, false, false, OrderSide::Buy)]
#[case::l3_synthetic(BookType::L3_MBO, false, true, false, OrderSide::Buy)]
#[case::l3_synthetic_consumption(BookType::L3_MBO, true, true, true, OrderSide::Sell)]
fn test_market_to_limit_first_price_level(
    #[case] book_type: BookType,
    #[case] liquidity_consumption: bool,
    #[case] synthetic: bool,
    #[case] deferred: bool,
    #[case] side: OrderSide,
    #[values(
        (TimeInForce::Fok, "2.000"),
        (TimeInForce::Fok, "3.000"),
        (TimeInForce::Gtc, "3.000"),
        (TimeInForce::Ioc, "2.000")
    )]
    scenario: (TimeInForce, &str),
) {
    let (mut engine, events) = custom_book_engine(book_type, liquidity_consumption, false);
    engine.book.clear(1, 1.into());
    let (first, deeper) = if side == OrderSide::Buy {
        (Price::from("100.00"), Price::from("101.00"))
    } else {
        (Price::from("101.00"), Price::from("100.00"))
    };
    let levels = if book_type == BookType::L3_MBO {
        vec![(first, "1.000"), (first, "1.000"), (deeper, "1.000")]
    } else {
        vec![(first, "2.000"), (deeper, "1.000")]
    };

    for (idx, (price, size)) in levels.into_iter().enumerate() {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(side.opposite(), price, Quantity::from(size), idx as u64 + 1),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    if synthetic {
        engine.set_fill_model(FillModelHandle::new(TestBookFillModel {
            book: engine.book.clone(),
        }));
    }

    if deferred {
        let captured = events.clone();
        engine.set_event_handler(Rc::new(move |event| captured.borrow_mut().push(event)));
    }

    let (time_in_force, quantity) = scenario;
    let quantity = Quantity::from(quantity);
    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .side(side)
        .quantity(quantity)
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    let canceled = time_in_force == TimeInForce::Fok && quantity > Quantity::from("2.000");
    let rests = time_in_force != TimeInForce::Fok && quantity > Quantity::from("2.000");
    assert_eq!(engine.core.order_exists(order.client_order_id()), rests);

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    let saved = events.borrow();
    let fills: Vec<_> = saved
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_px, fill.last_qty)),
            _ => None,
        })
        .collect();
    let expected = if canceled {
        vec![]
    } else if book_type == BookType::L3_MBO && (!liquidity_consumption || synthetic) {
        vec![(first, Quantity::from("1.000")); 2]
    } else {
        vec![(first, Quantity::from("2.000"))]
    };
    assert_eq!(fills, expected);
    assert_eq!(
        saved
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Updated(_)))
            .count(),
        usize::from(!canceled)
    );
    let cached = engine
        .cache
        .borrow()
        .try_order_owned(&order.client_order_id())
        .unwrap();
    assert_eq!(
        cached.status(),
        if canceled {
            OrderStatus::Canceled
        } else if rests {
            OrderStatus::Accepted
        } else {
            OrderStatus::Filled
        }
    );
    assert_eq!(
        cached.filled_qty(),
        Quantity::from(if canceled { "0.000" } else { "2.000" })
    );

    if !canceled {
        assert_eq!(cached.price(), Some(first));
    }

    let consumption = if side == OrderSide::Buy {
        &engine.ask_consumption
    } else {
        &engine.bid_consumption
    };

    if liquidity_consumption && !synthetic && !canceled {
        assert_eq!(consumption.len(), 1);
        assert_eq!(
            consumption[&first.raw()],
            (Quantity::from("2.000").raw(), Quantity::from("2.000").raw())
        );
    } else {
        assert!(consumption.is_empty());
    }
}

#[rstest]
#[case::filled("1.000", OrderStatus::Filled)]
#[case::canceled("0.500", OrderStatus::Canceled)]
fn test_market_to_limit_fok_caps_same_price_reduce_only_fills(
    #[case] size: &str,
    #[case] status: OrderStatus,
    #[values(false, true)] deferred: bool,
) {
    let (mut engine, events) = custom_book_engine(BookType::L3_MBO, false, false);
    engine.book.clear(1, 1.into());
    for (idx, (price, quantity)) in [("1501.00", size), ("1501.00", size), ("1502.00", "3.000")]
        .into_iter()
        .enumerate()
    {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from(price),
                    Quantity::from(quantity),
                    idx as u64 + 1,
                ),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    let (opening, fill) = pending_position_fill(
        &engine.instrument,
        PositionId::from("MTL-REDUCE-POS"),
        "MTL-REDUCE-OPEN",
        OrderSide::Sell,
        "1.500",
    );
    let position = Position::new(&engine.instrument, fill.clone());
    engine
        .cache
        .borrow_mut()
        .add_order(opening, Some(position.id), None, false)
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(fill))
        .unwrap();
    engine
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    if deferred {
        let captured = events.clone();
        engine.set_event_handler(Rc::new(move |event| captured.borrow_mut().push(event)));
    }

    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("MTL-REDUCE-CLOSE"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("3.000"))
        .time_in_force(TimeInForce::Fok)
        .reduce_only(true)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), Some(position.id), None, false)
        .unwrap();
    engine.process_order(&mut order, AccountId::from("SIM-001"));
    assert!(!engine.core.order_exists(order.client_order_id()));

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_px, fill.last_qty)),
            _ => None,
        })
        .collect();
    let expected = if status == OrderStatus::Filled {
        vec![
            (Price::from("1501.00"), Quantity::from("1.000")),
            (Price::from("1501.00"), Quantity::from("0.500")),
        ]
    } else {
        vec![]
    };
    assert_eq!(fills, expected);
    let cache = engine.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.status(), status);
    assert_eq!(
        cached.quantity(),
        Quantity::from(if status == OrderStatus::Filled {
            "1.500"
        } else {
            "3.000"
        })
    );
    assert_eq!(
        cached.filled_qty(),
        Quantity::from(if status == OrderStatus::Filled {
            "1.500"
        } else {
            "0.000"
        })
    );
}

#[rstest]
#[case::filled("2.000", OrderStatus::Filled)]
#[case::canceled("3.000", OrderStatus::Canceled)]
fn test_market_to_limit_fok_ignores_nonexecutable_fills(
    #[case] quantity: &str,
    #[case] status: OrderStatus,
) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    let order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(quantity))
        .time_in_force(TimeInForce::Fok)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    engine
        .account_ids
        .insert(order.trader_id(), AccountId::from("SIM-001"));
    let fills = [
        (Price::from("1500.00"), Quantity::from("0.000")),
        (Price::from("1500.001"), Quantity::from("1.000")),
        (Price::from("1501.00"), Quantity::from("0.0005")),
        (Price::from("1501.00"), Quantity::from("1.000")),
        (Price::from("1500.00"), Quantity::from("0.000")),
        (Price::from("1501.00"), Quantity::from("1.000")),
        (Price::from("1502.00"), Quantity::from("10.000")),
    ];
    engine
        .apply_fills(&order, &fills, LiquiditySide::Taker, None, None, None, true)
        .unwrap();
    let cached = engine
        .cache
        .borrow()
        .try_order_owned(&order.client_order_id())
        .unwrap();
    assert_eq!(cached.status(), status);
    assert_eq!(
        cached.filled_qty(),
        Quantity::from(if status == OrderStatus::Filled {
            "2.000"
        } else {
            "0.000"
        })
    );
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        if status == OrderStatus::Filled { 2 } else { 0 }
    );
}

#[rstest]
#[case::off_tick_l2("off_tick", BookType::L2_MBP, false, false, false, TimeInForce::Ioc)]
#[case::off_tick_consumption("off_tick", BookType::L2_MBP, true, false, false, TimeInForce::Ioc)]
#[case::off_tick_l3("off_tick", BookType::L3_MBO, false, false, false, TimeInForce::Ioc)]
#[case::off_tick_synthetic("off_tick", BookType::L3_MBO, true, true, false, TimeInForce::Ioc)]
#[case::off_tick_acknowledged("off_tick", BookType::L2_MBP, false, false, true, TimeInForce::Ioc)]
#[case::invalid_quantity(
    "invalid_quantity",
    BookType::L2_MBP,
    false,
    false,
    false,
    TimeInForce::Ioc
)]
#[case::filled("filled", BookType::L2_MBP, false, false, false, TimeInForce::Ioc)]
#[case::mixed_ioc("mixed", BookType::L3_MBO, false, false, false, TimeInForce::Ioc)]
#[case::mixed_gtc("mixed", BookType::L3_MBO, false, false, false, TimeInForce::Gtc)]
#[case::partial_ioc("partial", BookType::L2_MBP, false, false, false, TimeInForce::Ioc)]
#[case::empty_book("empty", BookType::L2_MBP, false, false, false, TimeInForce::Ioc)]
fn test_market_to_limit_normalized_fill_outcome(
    #[case] scenario: &str,
    #[case] book_type: BookType,
    #[case] liquidity_consumption: bool,
    #[case] synthetic: bool,
    #[case] use_market_order_acks: bool,
    #[case] time_in_force: TimeInForce,
    #[values(false, true)] deferred: bool,
) {
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.price_increment = Price::from("0.05");
    let instrument = InstrumentAny::CryptoPerpetual(instrument);
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        book_type,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig {
            liquidity_consumption,
            use_market_order_acks,
            ..Default::default()
        },
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred {
            cache.borrow_mut().update_order(&event).unwrap();
        }

        captured.borrow_mut().push(event);
    }));
    let levels = match scenario {
        "off_tick" => vec![("1501.01", "2.000")],
        "partial" => vec![("1501.00", "1.000")],
        "mixed" => vec![("1501.01", "1.000"), ("1501.05", "2.000")],
        "empty" => vec![],
        _ => vec![("1501.00", "2.000")],
    };

    for (index, (price, size)) in levels.into_iter().enumerate() {
        let size = if scenario == "invalid_quantity" {
            // Valid precision metadata with raw bits that cannot be normalized to it.
            Quantity::from_raw(Quantity::from("0.0001").raw(), 3)
        } else {
            Quantity::from(size)
        };
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                engine.instrument.id(),
                BookAction::Add,
                BookOrder::new(OrderSide::Sell, Price::from(price), size, index as u64 + 1),
                RecordFlag::F_LAST as u8,
                1,
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    if scenario == "off_tick" {
        assert_eq!(engine.book.best_ask_price(), Some(Price::from("1501.01")));
    }

    if synthetic {
        engine.set_fill_model(FillModelHandle::new(TestBookFillModel {
            book: engine.book.clone(),
        }));
    }

    let mut order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("2.000"))
        .time_in_force(time_in_force)
        .submit(true)
        .build();
    process_cached_order(&mut engine, &mut order, AccountId::from("SIM-001"));

    let canceled = matches!(scenario, "off_tick" | "invalid_quantity");
    let partial = matches!(scenario, "mixed" | "partial");
    let expected_status = if canceled {
        OrderStatus::Canceled
    } else if scenario == "empty" {
        OrderStatus::Rejected
    } else if partial {
        // Preserve the existing partial IOC remainder behavior in this focused fix.
        OrderStatus::Accepted
    } else {
        OrderStatus::Filled
    };
    assert_eq!(engine.core.order_exists(order.client_order_id()), partial);
    let mut expected_events = if use_market_order_acks {
        vec![OrderEventType::Accepted]
    } else {
        vec![]
    };
    expected_events.extend(if canceled {
        vec![OrderEventType::Canceled]
    } else if scenario == "empty" {
        vec![OrderEventType::Rejected]
    } else if partial {
        vec![
            OrderEventType::Updated,
            OrderEventType::Filled,
            OrderEventType::Accepted,
        ]
    } else {
        vec![OrderEventType::Updated, OrderEventType::Filled]
    });
    assert_eq!(
        events
            .borrow()
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        expected_events
    );

    if deferred {
        for event in events.borrow().iter() {
            engine.cache.borrow_mut().update_order(event).unwrap();
        }
    }

    let cached = engine
        .cache
        .borrow()
        .try_order_owned(&order.client_order_id())
        .unwrap();
    assert_eq!(cached.status(), expected_status);
    assert_eq!(
        cached.filled_qty(),
        Quantity::from(if canceled || scenario == "empty" {
            "0.000"
        } else if partial {
            "1.000"
        } else {
            "2.000"
        })
    );
    let expected_price = if canceled || scenario == "empty" {
        None
    } else {
        Some(Price::from(if scenario == "mixed" {
            "1501.05"
        } else {
            "1501.00"
        }))
    };
    assert_eq!(cached.price(), expected_price);
    assert_eq!(engine.core.order_exists(order.client_order_id()), partial);

    if partial {
        assert_eq!(
            engine
                .core
                .get_order(order.client_order_id())
                .unwrap()
                .limit_price,
            expected_price
        );
    }
}

#[rstest]
#[case::zero_only(false)]
#[case::zero_then_valid(true)]
fn test_market_to_limit_ioc_zero_fill_candidates(#[case] has_valid_fill: bool) {
    let (mut engine, events) = custom_book_engine(BookType::L2_MBP, false, false);
    let order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .instrument_id(engine.instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .time_in_force(TimeInForce::Ioc)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    engine
        .account_ids
        .insert(order.trader_id(), AccountId::from("SIM-001"));
    // Normal book ingestion rejects zero-size adds; exercise the consumer boundary directly.
    let mut fills = vec![(Price::from("1500.00"), Quantity::from("0.000"))];

    if has_valid_fill {
        fills.push((Price::from("1501.00"), Quantity::from("1.000")));
    }

    engine
        .apply_fills(&order, &fills, LiquiditySide::Taker, None, None, None, true)
        .unwrap();
    let expected_events = if has_valid_fill {
        vec![OrderEventType::Updated, OrderEventType::Filled]
    } else {
        vec![OrderEventType::Canceled]
    };
    assert_eq!(
        events
            .borrow()
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        expected_events
    );
    let cache = engine.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(
        cached.status(),
        if has_valid_fill {
            OrderStatus::Filled
        } else {
            OrderStatus::Canceled
        }
    );
    assert_eq!(
        cached.filled_qty(),
        Quantity::from(if has_valid_fill { "1.000" } else { "0.000" })
    );
    assert_eq!(
        cached.price(),
        has_valid_fill.then_some(Price::from("1501.00"))
    );
    assert!(!engine.core.order_exists(order.client_order_id()));
}

#[rstest]
#[case::working(None)]
#[case::canceled(Some(OrderEventType::Canceled))]
#[case::expired(Some(OrderEventType::Expired))]
fn test_restore_open_order_respects_oto_terminal(#[case] terminal: Option<OrderEventType>) {
    let (mut engine, _) = custom_book_engine(BookType::L2_MBP, false, false);
    engine.config.support_contingent_orders = true;
    let parent_id = ClientOrderId::from("OTO-PARENT");
    let child_id = ClientOrderId::from("OTO-CHILD");
    let mut parent = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(parent_id)
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("3.000"))
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .submit(true)
        .build();
    let mut child = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(engine.instrument.id())
        .client_order_id(child_id)
        .side(OrderSide::Sell)
        .price(Price::from("1600.00"))
        .quantity(Quantity::from("3.000"))
        .parent_order_id(parent_id)
        .submit(true)
        .build();
    let account_id = AccountId::from("SIM-001");

    for order in [&parent, &child] {
        engine
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    engine.process_order(&mut parent, account_id);
    engine.process_order(&mut child, account_id);
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    engine.set_event_handler(Rc::new(move |event| captured.borrow_mut().push(event)));
    match terminal {
        Some(OrderEventType::Expired) => engine.expire_order(&child),
        Some(_) => engine.cancel_order(&child, None),
        None => {
            let accepted =
                TestOrderEventStubs::accepted(&child, account_id, VenueOrderId::from("V-CHILD"));
            engine.cache.borrow_mut().update_order(&accepted).unwrap();
        }
    }

    let cached = engine.cache.borrow().try_order_owned(&child_id).unwrap();
    engine.restore_open_order(&cached, account_id);

    let expected_status = if terminal.is_some() {
        OrderStatus::Submitted
    } else {
        OrderStatus::Accepted
    };

    assert_eq!(cached.status(), expected_status);
    assert_eq!(engine.core.order_exists(child_id), terminal.is_none());
    assert!(engine.core.order_exists(parent_id));
    let terminals: Vec<_> = events
        .borrow()
        .iter()
        .map(|event| (event.client_order_id(), event.event_type()))
        .collect();

    let expected: Vec<_> = terminal.into_iter().map(|kind| (child_id, kind)).collect();
    assert_eq!(terminals, expected);
}
