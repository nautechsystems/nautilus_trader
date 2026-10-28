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

use nautilus_execution::models::fee::{FeeModel, FeeModelHandle};

use super::*;

#[rstest]
fn test_custom_fill_model_handle_is_called_by_market_fill() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let calls = Rc::new(Cell::new(0));

    let fill_model = FillModelHandle::new(RecordingFillModel {
        calls: Rc::clone(&calls),
    });

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        fill_model,
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache.clone(),
        Default::default(),
    );

    let quote = QuoteTick::new(
        instrument.id(),
        Price::from("1500.00"),
        Price::from("1501.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    engine.process_quote_tick(&quote);

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    cache_order(&cache, &order);
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    assert_eq!(calls.get(), 1);
}

#[rstest]
fn test_custom_fee_model_handle_is_called_by_fill_order() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let calls = Rc::new(Cell::new(0));
    let expected_commission = Money::from("1.23 USDT");

    let fee_model = FeeModelHandle::new(RecordingFeeModel {
        calls: Rc::clone(&calls),
        commission: expected_commission,
    });

    let cloned_fee_model = fee_model.clone();
    drop(fee_model);

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        cloned_fee_model,
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache.clone(),
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
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("1499.00"),
        Price::from("1500.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    ));
    cache_order(&cache, &order);
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let events = events.borrow();
    assert_eq!(events.len(), 1);

    let fill = match &events[0] {
        OrderEventAny::Filled(fill) => fill,
        event => panic!("Expected OrderFilled, was {event:?}"),
    };

    assert_eq!(calls.get(), 1);
    assert_eq!(fill.commission, Some(expected_commission));
}

#[rstest]
fn test_l1_custom_book_partial_market_respects_liquidity(
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(false, true)] one_sided: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let mut engine = build_caching_engine(
        instrument,
        BookType::L1_MBP,
        None,
        None,
        Some(AccountType::Margin),
        None,
    );
    let cache = engine.cache.clone();
    let events = Rc::new(RefCell::new(Vec::new()));
    let saved_events = events.clone();
    engine.set_event_handler(Rc::new(move |event| {
        cache.borrow_mut().update_order(&event).unwrap();
        saved_events.borrow_mut().push(event);
    }));

    let size = Quantity::from("10.000");
    let zero = Quantity::from("0.000");
    let (bid_size, ask_size) = match (one_sided, side) {
        (true, OrderSide::Buy) => (zero, size),
        (true, OrderSide::Sell) => (size, zero),
        (false, _) => (size, size),
    };

    engine.process_quote_tick(&QuoteTick::new(
        instrument_id,
        Price::from("1499.00"),
        Price::from("1501.00"),
        bid_size,
        ask_size,
        UnixNanos::from(1),
        UnixNanos::from(1),
    ));
    assert_eq!(
        engine.best_bid_price(),
        (!bid_size.is_zero()).then_some(Price::from("1499.00")),
    );
    assert_eq!(
        engine.best_ask_price(),
        (!ask_size.is_zero()).then_some(Price::from("1501.00")),
    );

    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    for (side, price) in [(OrderSide::Buy, "1499.00"), (OrderSide::Sell, "1501.00")] {
        book.add(
            BookOrder::new(side, Price::from(price), Quantity::from("1.000"), 0),
            0,
            1,
            UnixNanos::from(1),
        );
    }

    engine.set_fill_model(FillModelHandle::new(TestBookFillModel { book }));
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from("3.000"))
        .time_in_force(TimeInForce::Gtc)
        .submit(true)
        .build();
    engine
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let fills: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_px, fill.last_qty)),
            _ => None,
        })
        .collect();

    let price = Price::from(if side == OrderSide::Buy {
        "1501.00"
    } else {
        "1499.00"
    });

    assert_eq!(fills, vec![(price, Quantity::from("1.000"))]);
    let cache = engine.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.status(), OrderStatus::PartiallyFilled);
    assert_eq!(cached.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cached.leaves_qty(), Quantity::from("2.000"));
}

struct RecordingFeeModel {
    calls: Rc<Cell<u32>>,
    commission: Money,
}

impl FeeModel for RecordingFeeModel {
    fn get_commission(
        &self,
        _order: &OrderAny,
        _fill_quantity: Quantity,
        _fill_px: Price,
        _instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.commission)
    }
}

struct RecordingFillModel {
    calls: Rc<Cell<u32>>,
}

impl FillModel for RecordingFillModel {
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
        self.calls.set(self.calls.get() + 1);
        Ok(None)
    }
}

pub(super) struct TestBookFillModel {
    pub(super) book: OrderBook,
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
