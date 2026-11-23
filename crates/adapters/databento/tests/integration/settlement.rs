// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{cell::RefCell, rc::Rc};

use databento::dbn::{InstrumentDefMsg, Publisher, RecordHeader, record::str_to_c_chars, rtype};
use nautilus_common::{cache::Cache, clock::VirtualClock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_databento::decode::decode_instrument_def_msg;
use nautilus_execution::{
    matching_engine::OrderMatchingEngine,
    models::{
        fee::{FeeModelHandle, MakerTakerFeeModel},
        fill::FillModelHandle,
    },
};
use nautilus_model::{
    data::TradeTick,
    enums::{AccountType, AggressorSide, BookType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;

#[rstest]
fn test_glbx_decoded_weekly_option_delivers_one_future_per_contract() {
    let ts_recv = 1_794_960_000_000_000_000;
    let option_id = InstrumentId::from("E3DX6 C6800.GLBX");
    let future_id = InstrumentId::from("ESZ6.GLBX");
    let publisher_id = Publisher::GlbxMdp3Glbx as u16;
    let future_header =
        RecordHeader::new::<InstrumentDefMsg>(rtype::INSTRUMENT_DEF, publisher_id, 10_252, ts_recv);

    let future = InstrumentDefMsg {
        hd: future_header,
        ts_recv,
        raw_symbol: str_to_c_chars("ESZ6").unwrap(),
        asset: str_to_c_chars("ES").unwrap(),
        exchange: str_to_c_chars("XCME").unwrap(),
        currency: str_to_c_chars("USD").unwrap(),
        cfi: str_to_c_chars("FFIXSX").unwrap(),
        instrument_class: b'F' as _,
        expiration: 1_797_604_200_000_000_000,
        min_price_increment: 250_000_000,
        unit_of_measure_qty: 50_000_000_000,
        min_lot_size_round_lot: 1,
        ..Default::default()
    };

    let option_header = RecordHeader::new::<InstrumentDefMsg>(
        rtype::INSTRUMENT_DEF,
        publisher_id,
        42_800_789,
        ts_recv,
    );

    let option = InstrumentDefMsg {
        hd: option_header,
        raw_symbol: str_to_c_chars("E3DX6 C6800").unwrap(),
        asset: str_to_c_chars("E3D").unwrap(),
        underlying: str_to_c_chars("ESZ6").unwrap(),
        underlying_id: future_header.instrument_id,
        cfi: str_to_c_chars("OCEFPS").unwrap(),
        instrument_class: b'C' as _,
        expiration: 1_795_122_000_000_000_000,
        strike_price: 6_800_000_000_000,
        strike_price_currency: str_to_c_chars("USD").unwrap(),
        ..future
    };

    let missing_future_multiplier = InstrumentDefMsg {
        unit_of_measure_qty: databento::dbn::UNDEF_PRICE,
        ..future
    };

    let skipped =
        decode_instrument_def_msg(&missing_future_multiplier, future_id, None, None).unwrap();
    let future = decode_instrument_def_msg(&future, future_id, None, None)
        .unwrap()
        .unwrap();

    let InstrumentAny::OptionContract(mut option) =
        decode_instrument_def_msg(&option, option_id, None, None)
            .unwrap()
            .unwrap()
    else {
        panic!("Expected OptionContract");
    };

    option.underlying_instrument_id = Some(future_id);
    let expiry = option.expiration_ns;
    let option = InstrumentAny::OptionContract(option);
    let now = UnixNanos::from(ts_recv);
    let qty = Quantity::from(2);
    let usd = Currency::USD();

    let opening_fill = OrderFilled {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("S-001"),
        instrument_id: option_id,
        client_order_id: ClientOrderId::from("OPEN-OPTION"),
        venue_order_id: VenueOrderId::from("OPEN-VENUE"),
        account_id: AccountId::from("GLBX-001"),
        trade_id: TradeId::from("OPEN-TRADE"),
        order_side: OrderSide::Buy,
        order_type: OrderType::Market,
        last_qty: qty,
        last_px: Price::from("5.00"),
        currency: usd,
        liquidity_side: LiquiditySide::Taker,
        event_id: UUID4::new(),
        ts_event: now,
        ts_init: now,
        reconciliation: false,
        position_id: Some(PositionId::from("OPTION-POSITION")),
        commission: Some(Money::zero(usd)),
        info: None,
        causation_id: None,
    };

    let position = Position::new(&option, opening_fill);
    let spot = Price::from("6810.00");
    let trade_id = TradeId::from("UNDERLYING");

    let trade = TradeTick::new(
        future_id,
        spot,
        qty,
        AggressorSide::NoAggressor,
        trade_id,
        now,
        now,
    );

    let mut cache = Cache::default();
    cache.add_instrument(future.clone()).unwrap();
    cache.add_instrument(option.clone()).unwrap();
    cache.add_trade(trade).unwrap();
    cache
        .add_position_without_order(&position, OmsType::Netting)
        .unwrap();
    let cache = Rc::new(RefCell::new(cache));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    clock.borrow_mut().set_time(expiry);
    let multiplier = option.multiplier();
    let fee_model = FeeModelHandle::new(MakerTakerFeeModel::zero());
    let fill_model = FillModelHandle::default();

    let mut engine = OrderMatchingEngine::new(
        option,
        1,
        fill_model,
        fee_model,
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        cache,
        Default::default(),
    );

    let events = Rc::new(RefCell::new(Vec::new()));
    let fills = events.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if let OrderEventAny::Filled(fill) = event {
            fills.borrow_mut().push((
                fill.instrument_id,
                fill.order_side,
                fill.last_qty,
                fill.last_px,
            ));
        }
    }));

    engine.process_instrument_expiration(expiry);

    assert!(skipped.is_none());
    assert!(engine.is_expiration_processed());
    assert_eq!(multiplier, Quantity::from(50));
    assert_eq!(future.multiplier(), Quantity::from(50));
    assert_eq!(
        *events.borrow(),
        [
            (option_id, OrderSide::Sell, qty, Price::from("0.00")),
            (future_id, OrderSide::Buy, qty, Price::from("6800.00")),
        ]
    );
}
