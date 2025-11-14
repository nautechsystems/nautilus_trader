// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Cap'n Proto serialization integration tests for market data types.

#![cfg(feature = "capnp")]

use nautilus_model::{
    data::{
        FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
        QuoteTick, TradeTick,
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        depth::OrderBookDepth10,
        order::BookOrder,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, InstrumentCloseType,
        MarketStatusAction, OrderSide, PriceType,
    },
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_serialization::capnp::{FromCapnp, ToCapnp, market_capnp};
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

#[rstest]
fn test_quote_tick_roundtrip() {
    let quote = QuoteTick {
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        bid_price: Price::from("100.50"),
        ask_price: Price::from("100.55"),
        bid_size: Quantity::from("10.5"),
        ask_size: Quantity::from("8.3"),
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::quote_tick::Builder>();
    quote.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::quote_tick::Reader>()
        .unwrap();
    let decoded = QuoteTick::from_capnp(root).unwrap();

    assert_eq!(quote.instrument_id, decoded.instrument_id);
    assert_eq!(quote.bid_price, decoded.bid_price);
    assert_eq!(quote.ask_price, decoded.ask_price);
    assert_eq!(quote.bid_size, decoded.bid_size);
    assert_eq!(quote.ask_size, decoded.ask_size);
    assert_eq!(quote.ts_event, decoded.ts_event);
    assert_eq!(quote.ts_init, decoded.ts_init);
}

#[rstest]
fn test_trade_tick_roundtrip() {
    let trade = TradeTick {
        instrument_id: InstrumentId::from("ETHUSDT.BINANCE"),
        price: Price::from("2500.75"),
        size: Quantity::from("1.5"),
        aggressor_side: AggressorSide::Buyer,
        trade_id: TradeId::from("12345"),
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::trade_tick::Builder>();
    trade.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::trade_tick::Reader>()
        .unwrap();
    let decoded = TradeTick::from_capnp(root).unwrap();

    assert_eq!(trade.instrument_id, decoded.instrument_id);
    assert_eq!(trade.price, decoded.price);
    assert_eq!(trade.size, decoded.size);
    assert_eq!(trade.aggressor_side, decoded.aggressor_side);
    assert_eq!(trade.trade_id, decoded.trade_id);
    assert_eq!(trade.ts_event, decoded.ts_event);
    assert_eq!(trade.ts_init, decoded.ts_init);
}

#[rstest]
#[case(AggressorSide::NoAggressor)]
#[case(AggressorSide::Buyer)]
#[case(AggressorSide::Seller)]
fn test_trade_tick_all_aggressor_sides(#[case] aggressor_side: AggressorSide) {
    let trade = TradeTick {
        instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
        price: Price::from("50000.00"),
        size: Quantity::from("0.1"),
        aggressor_side,
        trade_id: TradeId::from("T123"),
        ts_event: 1000000.into(),
        ts_init: 1000001.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::trade_tick::Builder>();
    trade.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::trade_tick::Reader>()
        .unwrap();
    let decoded = TradeTick::from_capnp(root).unwrap();

    assert_eq!(trade.aggressor_side, decoded.aggressor_side);
}

#[rstest]
fn test_mark_price_update_roundtrip() {
    let mark_price = MarkPriceUpdate {
        instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        value: Price::from("50000.50"),
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::mark_price_update::Builder>();
    mark_price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::mark_price_update::Reader>()
        .unwrap();
    let decoded = MarkPriceUpdate::from_capnp(root).unwrap();

    assert_eq!(mark_price.instrument_id, decoded.instrument_id);
    assert_eq!(mark_price.value, decoded.value);
    assert_eq!(mark_price.ts_event, decoded.ts_event);
    assert_eq!(mark_price.ts_init, decoded.ts_init);
}

#[rstest]
fn test_index_price_update_roundtrip() {
    let index_price = IndexPriceUpdate {
        instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        value: Price::from("50125.75"),
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::index_price_update::Builder>();
    index_price.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::index_price_update::Reader>()
        .unwrap();
    let decoded = IndexPriceUpdate::from_capnp(root).unwrap();

    assert_eq!(index_price.instrument_id, decoded.instrument_id);
    assert_eq!(index_price.value, decoded.value);
    assert_eq!(index_price.ts_event, decoded.ts_event);
    assert_eq!(index_price.ts_init, decoded.ts_init);
}

#[rstest]
fn test_funding_rate_update_with_next_funding_time() {
    let funding_rate = FundingRateUpdate {
        instrument_id: InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        rate: dec!(0.0001),
        next_funding_ns: Some(1234567890.into()),
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::funding_rate_update::Builder>();
    funding_rate.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::funding_rate_update::Reader>()
        .unwrap();
    let decoded = FundingRateUpdate::from_capnp(root).unwrap();

    assert_eq!(funding_rate.instrument_id, decoded.instrument_id);
    assert_eq!(funding_rate.rate, decoded.rate);
    assert_eq!(funding_rate.next_funding_ns, decoded.next_funding_ns);
    assert_eq!(funding_rate.ts_event, decoded.ts_event);
    assert_eq!(funding_rate.ts_init, decoded.ts_init);
}

#[rstest]
fn test_funding_rate_update_without_next_funding_time() {
    let funding_rate = FundingRateUpdate {
        instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        rate: dec!(-0.00025),
        next_funding_ns: None,
        ts_event: 9876543210.into(),
        ts_init: 9876543211.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::funding_rate_update::Builder>();
    funding_rate.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::funding_rate_update::Reader>()
        .unwrap();
    let decoded = FundingRateUpdate::from_capnp(root).unwrap();

    assert_eq!(funding_rate.instrument_id, decoded.instrument_id);
    assert_eq!(funding_rate.rate, decoded.rate);
    assert_eq!(funding_rate.next_funding_ns, None);
    assert_eq!(funding_rate.ts_event, decoded.ts_event);
    assert_eq!(funding_rate.ts_init, decoded.ts_init);
}

#[rstest]
fn test_funding_rate_update_with_large_decimal() {
    let funding_rate = FundingRateUpdate {
        instrument_id: InstrumentId::from("SOLUSDT-PERP.BINANCE"),
        rate: dec!(0.123456789012345678),
        next_funding_ns: Some(5555555555.into()),
        ts_event: 1111111111.into(),
        ts_init: 1111111112.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::funding_rate_update::Builder>();
    funding_rate.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::funding_rate_update::Reader>()
        .unwrap();
    let decoded = FundingRateUpdate::from_capnp(root).unwrap();

    assert_eq!(funding_rate.instrument_id, decoded.instrument_id);
    assert_eq!(funding_rate.rate, decoded.rate);
    assert_eq!(funding_rate.next_funding_ns, decoded.next_funding_ns);
    assert_eq!(funding_rate.ts_event, decoded.ts_event);
    assert_eq!(funding_rate.ts_init, decoded.ts_init);
}

#[rstest]
#[case(InstrumentCloseType::EndOfSession)]
#[case(InstrumentCloseType::ContractExpired)]
fn test_instrument_close_all_types(#[case] close_type: InstrumentCloseType) {
    let close = InstrumentClose {
        instrument_id: InstrumentId::from("ES.GLOBEX"),
        close_price: Price::from("4500.25"),
        close_type,
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::instrument_close::Builder>();
    close.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::instrument_close::Reader>()
        .unwrap();
    let decoded = InstrumentClose::from_capnp(root).unwrap();

    assert_eq!(close.instrument_id, decoded.instrument_id);
    assert_eq!(close.close_price, decoded.close_price);
    assert_eq!(close.close_type, decoded.close_type);
    assert_eq!(close.ts_event, decoded.ts_event);
    assert_eq!(close.ts_init, decoded.ts_init);
}

#[rstest]
#[case(MarketStatusAction::None)]
#[case(MarketStatusAction::PreOpen)]
#[case(MarketStatusAction::PreCross)]
#[case(MarketStatusAction::Quoting)]
#[case(MarketStatusAction::Cross)]
#[case(MarketStatusAction::Rotation)]
#[case(MarketStatusAction::NewPriceIndication)]
#[case(MarketStatusAction::Trading)]
#[case(MarketStatusAction::Halt)]
#[case(MarketStatusAction::Pause)]
#[case(MarketStatusAction::Suspend)]
#[case(MarketStatusAction::PreClose)]
#[case(MarketStatusAction::Close)]
#[case(MarketStatusAction::PostClose)]
#[case(MarketStatusAction::ShortSellRestrictionChange)]
#[case(MarketStatusAction::NotAvailableForTrading)]
fn test_instrument_status_all_actions(#[case] action: MarketStatusAction) {
    let status = InstrumentStatus {
        instrument_id: InstrumentId::from("AAPL.NASDAQ"),
        action,
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
        reason: Some(Ustr::from("Market halt due to volatility")),
        trading_event: Some(Ustr::from("LUDP")),
        is_trading: Some(false),
        is_quoting: Some(true),
        is_short_sell_restricted: Some(true),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::instrument_status::Builder>();
    status.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::instrument_status::Reader>()
        .unwrap();
    let decoded = InstrumentStatus::from_capnp(root).unwrap();

    assert_eq!(status.instrument_id, decoded.instrument_id);
    assert_eq!(status.action, decoded.action);
    assert_eq!(status.reason, decoded.reason);
    assert_eq!(status.trading_event, decoded.trading_event);
    assert_eq!(status.is_trading, decoded.is_trading);
    assert_eq!(status.is_quoting, decoded.is_quoting);
    assert_eq!(
        status.is_short_sell_restricted,
        decoded.is_short_sell_restricted
    );
    assert_eq!(status.ts_event, decoded.ts_event);
    assert_eq!(status.ts_init, decoded.ts_init);
}

#[rstest]
fn test_instrument_status_with_no_optional_fields() {
    let status = InstrumentStatus {
        instrument_id: InstrumentId::from("TSLA.NASDAQ"),
        action: MarketStatusAction::Trading,
        ts_event: 1234567890.into(),
        ts_init: 1234567891.into(),
        reason: None,
        trading_event: None,
        is_trading: None,
        is_quoting: None,
        is_short_sell_restricted: None,
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::instrument_status::Builder>();
    status.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::instrument_status::Reader>()
        .unwrap();
    let decoded = InstrumentStatus::from_capnp(root).unwrap();

    assert_eq!(status.instrument_id, decoded.instrument_id);
    assert_eq!(status.action, decoded.action);
    assert_eq!(status.reason, None);
    assert_eq!(status.trading_event, None);
    assert_eq!(decoded.is_trading, Some(false));
    assert_eq!(decoded.is_quoting, Some(false));
    assert_eq!(decoded.is_short_sell_restricted, Some(false));
    assert_eq!(status.ts_event, decoded.ts_event);
    assert_eq!(status.ts_init, decoded.ts_init);
}

#[rstest]
fn test_instrument_status_with_empty_strings() {
    let status = InstrumentStatus {
        instrument_id: InstrumentId::from("MSFT.NASDAQ"),
        action: MarketStatusAction::PreOpen,
        ts_event: 5555555555.into(),
        ts_init: 5555555556.into(),
        reason: Some(Ustr::from("")),
        trading_event: Some(Ustr::from("")),
        is_trading: Some(true),
        is_quoting: Some(false),
        is_short_sell_restricted: Some(false),
    };

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::instrument_status::Builder>();
    status.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::instrument_status::Reader>()
        .unwrap();
    let decoded = InstrumentStatus::from_capnp(root).unwrap();

    assert_eq!(status.instrument_id, decoded.instrument_id);
    assert_eq!(status.action, decoded.action);
    assert_eq!(decoded.reason, None);
    assert_eq!(decoded.trading_event, None);
    assert_eq!(status.is_trading, decoded.is_trading);
    assert_eq!(status.is_quoting, decoded.is_quoting);
    assert_eq!(
        status.is_short_sell_restricted,
        decoded.is_short_sell_restricted
    );
    assert_eq!(status.ts_event, decoded.ts_event);
    assert_eq!(status.ts_init, decoded.ts_init);
}

#[rstest]
#[case(1, BarAggregation::Tick, PriceType::Last)]
#[case(5, BarAggregation::Minute, PriceType::Bid)]
#[case(15, BarAggregation::Minute, PriceType::Ask)]
#[case(1, BarAggregation::Hour, PriceType::Mid)]
#[case(4, BarAggregation::Hour, PriceType::Last)]
#[case(1, BarAggregation::Day, PriceType::Last)]
#[case(100, BarAggregation::Volume, PriceType::Last)]
#[case(1000, BarAggregation::Value, PriceType::Last)]
#[case(10, BarAggregation::Second, PriceType::Last)]
fn test_bar_specification_all_combinations(
    #[case] step: usize,
    #[case] aggregation: BarAggregation,
    #[case] price_type: PriceType,
) {
    let spec = BarSpecification::new(step, aggregation, price_type);

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar_spec::Builder>();
    spec.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar_spec::Reader>().unwrap();
    let decoded = BarSpecification::from_capnp(root).unwrap();

    assert_eq!(spec.step, decoded.step);
    assert_eq!(spec.aggregation, decoded.aggregation);
    assert_eq!(spec.price_type, decoded.price_type);
}

#[rstest]
#[case(AggregationSource::External)]
#[case(AggregationSource::Internal)]
fn test_bar_type_all_aggregation_sources(#[case] aggregation_source: AggregationSource) {
    let bar_type = BarType::new(
        InstrumentId::from("BTCUSDT.BINANCE"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        aggregation_source,
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar_type::Builder>();
    bar_type.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar_type::Reader>().unwrap();
    let decoded = BarType::from_capnp(root).unwrap();

    assert_eq!(bar_type.instrument_id(), decoded.instrument_id());
    assert_eq!(bar_type.spec(), decoded.spec());
    assert_eq!(bar_type.aggregation_source(), decoded.aggregation_source());
}

#[rstest]
fn test_bar_type_with_different_instruments() {
    let bar_type1 = BarType::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        BarSpecification::new(5, BarAggregation::Minute, PriceType::Bid),
        AggregationSource::External,
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar_type::Builder>();
    bar_type1.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar_type::Reader>().unwrap();
    let decoded = BarType::from_capnp(root).unwrap();

    assert_eq!(bar_type1, decoded);
}

#[rstest]
fn test_bar_roundtrip() {
    let bar_type = BarType::new(
        InstrumentId::from("AAPL.XNAS"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::Internal,
    );
    let bar = Bar::new(
        bar_type,
        Price::from("150.00"),
        Price::from("152.50"),
        Price::from("149.75"),
        Price::from("151.25"),
        Quantity::from("100000"),
        1234567890.into(),
        1234567891.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar::Builder>();
    bar.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar::Reader>().unwrap();
    let decoded = Bar::from_capnp(root).unwrap();

    assert_eq!(bar.bar_type, decoded.bar_type);
    assert_eq!(bar.open, decoded.open);
    assert_eq!(bar.high, decoded.high);
    assert_eq!(bar.low, decoded.low);
    assert_eq!(bar.close, decoded.close);
    assert_eq!(bar.volume, decoded.volume);
    assert_eq!(bar.ts_event, decoded.ts_event);
    assert_eq!(bar.ts_init, decoded.ts_init);
}

#[rstest]
fn test_bar_with_hour_aggregation() {
    let bar_type = BarType::new(
        InstrumentId::from("EURUSD.FXCM"),
        BarSpecification::new(4, BarAggregation::Hour, PriceType::Mid),
        AggregationSource::External,
    );
    let bar = Bar::new(
        bar_type,
        Price::from("1.10000"),
        Price::from("1.10250"),
        Price::from("1.09850"),
        Price::from("1.10125"),
        Quantity::from("5000000"),
        9999999999.into(),
        9999999999.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar::Builder>();
    bar.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar::Reader>().unwrap();
    let decoded = Bar::from_capnp(root).unwrap();

    assert_eq!(bar.bar_type, decoded.bar_type);
    assert_eq!(bar.open, decoded.open);
    assert_eq!(bar.high, decoded.high);
    assert_eq!(bar.low, decoded.low);
    assert_eq!(bar.close, decoded.close);
    assert_eq!(bar.volume, decoded.volume);
    assert_eq!(bar.ts_event, decoded.ts_event);
    assert_eq!(bar.ts_init, decoded.ts_init);
}

#[rstest]
fn test_bar_with_tick_aggregation() {
    let bar_type = BarType::new(
        InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        BarSpecification::new(100, BarAggregation::Tick, PriceType::Last),
        AggregationSource::Internal,
    );
    let bar = Bar::new(
        bar_type,
        Price::from("50000.00"),
        Price::from("50500.00"),
        Price::from("49800.00"),
        Price::from("50250.00"),
        Quantity::from("15.5"),
        1111111111.into(),
        1111111112.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar::Builder>();
    bar.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar::Reader>().unwrap();
    let decoded = Bar::from_capnp(root).unwrap();

    assert_eq!(bar, decoded);
}

#[rstest]
fn test_bar_with_volume_aggregation() {
    let bar_type = BarType::new(
        InstrumentId::from("ES.GLOBEX"),
        BarSpecification::new(10000, BarAggregation::Volume, PriceType::Last),
        AggregationSource::External,
    );
    let bar = Bar::new(
        bar_type,
        Price::from("4500.00"),
        Price::from("4510.25"),
        Price::from("4498.50"),
        Price::from("4505.75"),
        Quantity::from("10000"),
        7777777777.into(),
        7777777778.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::bar::Builder>();
    bar.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader.get_root::<market_capnp::bar::Reader>().unwrap();
    let decoded = Bar::from_capnp(root).unwrap();

    assert_eq!(bar, decoded);
}

#[rstest]
#[case(OrderSide::Buy)]
#[case(OrderSide::Sell)]
fn test_book_order_all_sides(#[case] side: OrderSide) {
    let order = BookOrder::new(side, Price::from("100.50"), Quantity::from("10.5"), 123456);

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::book_order::Builder>();
    order.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::book_order::Reader>()
        .unwrap();
    let decoded = BookOrder::from_capnp(root).unwrap();

    assert_eq!(order.side, decoded.side);
    assert_eq!(order.price, decoded.price);
    assert_eq!(order.size, decoded.size);
    assert_eq!(order.order_id, decoded.order_id);
}

#[rstest]
#[case(BookAction::Add)]
#[case(BookAction::Update)]
#[case(BookAction::Delete)]
#[case(BookAction::Clear)]
fn test_order_book_delta_all_actions(#[case] action: BookAction) {
    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("50000.00"),
        Quantity::from("1.5"),
        789,
    );
    let delta = OrderBookDelta::new(
        InstrumentId::from("BTCUSDT.BINANCE"),
        action,
        order,
        0,
        0,
        1234567890.into(),
        1234567891.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_delta::Builder>();
    delta.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::order_book_delta::Reader>()
        .unwrap();
    let decoded = OrderBookDelta::from_capnp(root).unwrap();

    assert_eq!(delta.instrument_id, decoded.instrument_id);
    assert_eq!(delta.action, decoded.action);
    assert_eq!(delta.order.side, decoded.order.side);
    assert_eq!(delta.order.price, decoded.order.price);
    assert_eq!(delta.order.size, decoded.order.size);
    assert_eq!(delta.order.order_id, decoded.order.order_id);
    assert_eq!(delta.flags, decoded.flags);
    assert_eq!(delta.sequence, decoded.sequence);
    assert_eq!(delta.ts_event, decoded.ts_event);
    assert_eq!(delta.ts_init, decoded.ts_init);
}

#[rstest]
fn test_order_book_deltas_with_multiple_deltas() {
    let order1 = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("5.0"),
        1,
    );
    let order2 = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from("3.0"),
        2,
    );
    let order3 = BookOrder::new(
        OrderSide::Buy,
        Price::from("99.50"),
        Quantity::from("10.0"),
        3,
    );

    let delta1 = OrderBookDelta::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        BookAction::Add,
        order1,
        0,
        1,
        1000000.into(),
        1000001.into(),
    );
    let delta2 = OrderBookDelta::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        BookAction::Update,
        order2,
        0,
        2,
        1000002.into(),
        1000003.into(),
    );
    let delta3 = OrderBookDelta::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        BookAction::Delete,
        order3,
        0,
        3,
        1000004.into(),
        1000005.into(),
    );

    let deltas = OrderBookDeltas::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        vec![delta1, delta2, delta3],
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_deltas::Builder>();
    deltas.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::order_book_deltas::Reader>()
        .unwrap();
    let decoded = OrderBookDeltas::from_capnp(root).unwrap();

    assert_eq!(deltas.instrument_id, decoded.instrument_id);
    assert_eq!(deltas.deltas.len(), decoded.deltas.len());
    assert_eq!(deltas.deltas.len(), 3);

    // Verify first delta
    assert_eq!(deltas.deltas[0].action, BookAction::Add);
    assert_eq!(deltas.deltas[0].action, decoded.deltas[0].action);
    assert_eq!(deltas.deltas[0].order.price, decoded.deltas[0].order.price);

    // Verify second delta
    assert_eq!(deltas.deltas[1].action, BookAction::Update);
    assert_eq!(deltas.deltas[1].action, decoded.deltas[1].action);
    assert_eq!(deltas.deltas[1].order.price, decoded.deltas[1].order.price);

    // Verify third delta
    assert_eq!(deltas.deltas[2].action, BookAction::Delete);
    assert_eq!(deltas.deltas[2].action, decoded.deltas[2].action);
    assert_eq!(deltas.deltas[2].order.price, decoded.deltas[2].order.price);

    assert_eq!(deltas.flags, decoded.flags);
    assert_eq!(deltas.sequence, decoded.sequence);
    assert_eq!(deltas.ts_event, decoded.ts_event);
    assert_eq!(deltas.ts_init, decoded.ts_init);
}

#[rstest]
fn test_order_book_depth10_roundtrip() {
    use nautilus_model::data::order::NULL_ORDER;

    let mut bids = [NULL_ORDER; 10];
    let mut asks = [NULL_ORDER; 10];

    // Populate 5 bid levels
    bids[0] = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("10.0"),
        0,
    );
    bids[1] = BookOrder::new(
        OrderSide::Buy,
        Price::from("99.50"),
        Quantity::from("5.0"),
        0,
    );
    bids[2] = BookOrder::new(
        OrderSide::Buy,
        Price::from("99.00"),
        Quantity::from("8.0"),
        0,
    );
    bids[3] = BookOrder::new(
        OrderSide::Buy,
        Price::from("98.50"),
        Quantity::from("12.0"),
        0,
    );
    bids[4] = BookOrder::new(
        OrderSide::Buy,
        Price::from("98.00"),
        Quantity::from("6.0"),
        0,
    );

    // Populate 5 ask levels
    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("100.50"),
        Quantity::from("9.0"),
        0,
    );
    asks[1] = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.00"),
        Quantity::from("7.0"),
        0,
    );
    asks[2] = BookOrder::new(
        OrderSide::Sell,
        Price::from("101.50"),
        Quantity::from("11.0"),
        0,
    );
    asks[3] = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.00"),
        Quantity::from("4.0"),
        0,
    );
    asks[4] = BookOrder::new(
        OrderSide::Sell,
        Price::from("102.50"),
        Quantity::from("15.0"),
        0,
    );

    let depth = OrderBookDepth10::new(
        InstrumentId::from("BTCUSDT.BINANCE"),
        bids,
        asks,
        [5, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [5, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
        100,
        1234567890.into(),
        1234567891.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_depth10::Builder>();
    depth.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::order_book_depth10::Reader>()
        .unwrap();
    let decoded = OrderBookDepth10::from_capnp(root).unwrap();

    assert_eq!(depth.instrument_id, decoded.instrument_id);

    // Verify bid levels (first 5 should match, rest should be NULL_ORDER)
    for i in 0..5 {
        assert_eq!(depth.bids[i].side, decoded.bids[i].side);
        assert_eq!(depth.bids[i].price, decoded.bids[i].price);
        assert_eq!(depth.bids[i].size, decoded.bids[i].size);
    }

    // Verify ask levels (first 5 should match, rest should be NULL_ORDER)
    for i in 0..5 {
        assert_eq!(depth.asks[i].side, decoded.asks[i].side);
        assert_eq!(depth.asks[i].price, decoded.asks[i].price);
        assert_eq!(depth.asks[i].size, decoded.asks[i].size);
    }

    assert_eq!(depth.bid_counts, decoded.bid_counts);
    assert_eq!(depth.ask_counts, decoded.ask_counts);
    assert_eq!(depth.flags, decoded.flags);
    assert_eq!(depth.sequence, decoded.sequence);
    assert_eq!(depth.ts_event, decoded.ts_event);
    assert_eq!(depth.ts_init, decoded.ts_init);
}

#[rstest]
fn test_order_book_depth10_with_partial_levels() {
    use nautilus_model::data::order::NULL_ORDER;

    let mut bids = [NULL_ORDER; 10];
    let mut asks = [NULL_ORDER; 10];

    // Only populate 2 bid levels
    bids[0] = BookOrder::new(
        OrderSide::Buy,
        Price::from("50000.00"),
        Quantity::from("1.5"),
        0,
    );
    bids[1] = BookOrder::new(
        OrderSide::Buy,
        Price::from("49999.50"),
        Quantity::from("2.0"),
        0,
    );

    // Only populate 2 ask levels
    asks[0] = BookOrder::new(
        OrderSide::Sell,
        Price::from("50000.50"),
        Quantity::from("1.2"),
        0,
    );
    asks[1] = BookOrder::new(
        OrderSide::Sell,
        Price::from("50001.00"),
        Quantity::from("3.5"),
        0,
    );

    let depth = OrderBookDepth10::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
        bids,
        asks,
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        0,
        50,
        5555555555.into(),
        5555555556.into(),
    );

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<market_capnp::order_book_depth10::Builder>();
    depth.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<market_capnp::order_book_depth10::Reader>()
        .unwrap();
    let decoded = OrderBookDepth10::from_capnp(root).unwrap();

    assert_eq!(depth.instrument_id, decoded.instrument_id);

    // Verify only first 2 bid levels
    for i in 0..2 {
        assert_eq!(depth.bids[i].side, decoded.bids[i].side);
        assert_eq!(depth.bids[i].price, decoded.bids[i].price);
        assert_eq!(depth.bids[i].size, decoded.bids[i].size);
    }

    // Verify only first 2 ask levels
    for i in 0..2 {
        assert_eq!(depth.asks[i].side, decoded.asks[i].side);
        assert_eq!(depth.asks[i].price, decoded.asks[i].price);
        assert_eq!(depth.asks[i].size, decoded.asks[i].size);
    }

    assert_eq!(depth.bid_counts, decoded.bid_counts);
    assert_eq!(depth.ask_counts, decoded.ask_counts);
    assert_eq!(depth.flags, decoded.flags);
    assert_eq!(depth.sequence, decoded.sequence);
    assert_eq!(depth.ts_event, decoded.ts_event);
    assert_eq!(depth.ts_init, decoded.ts_init);
}
