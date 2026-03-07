// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use nautilus_bitget::websocket::parse::{
    BitgetBookState, parse_public_book, parse_public_book_deltas, parse_public_trade_tick,
    parse_public_bars, parse_public_candle, parse_public_funding_rate, parse_public_index_price,
    parse_public_mark_price, parse_public_quote_tick, parse_public_ticker, parse_public_trades,
};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    enums::AggregationSource,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};

fn make_spot_instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::new(Symbol::new("BTCUSDT"), Venue::new("BITGET")),
        Symbol::new("BTCUSDT"),
        Currency::get_or_create_crypto_with_context("BTC", Some("test base")),
        Currency::get_or_create_crypto_with_context("USDT", Some("test quote")),
        2,
        2,
        Price::from("0.01"),
        Quantity::from("0.01"),
        None,
        Some(Quantity::from("0.01")),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    ))
}

fn make_perp_instrument() -> InstrumentAny {
    let quote = Currency::get_or_create_crypto_with_context("USDT", Some("test quote"));
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        InstrumentId::new(Symbol::new("BTCUSDT-PERP"), Venue::new("BITGET")),
        Symbol::new("BTCUSDT-PERP"),
        Currency::get_or_create_crypto_with_context("BTC", Some("test base")),
        quote,
        quote,
        false,
        2,
        2,
        Price::from("0.01"),
        Quantity::from("0.01"),
        None,
        Some(Quantity::from("0.01")),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::from(1_u64),
        UnixNanos::from(1_u64),
    ))
}

#[test]
fn snapshot_parses_into_valid_book_state_and_deltas() {
    let input = include_str!("../test_data/ws_public_depth_snapshot.json");
    let msg = parse_public_book(input).expect("snapshot fixture should deserialize");
    let instrument = make_spot_instrument();
    let mut state = BitgetBookState::default();

    state
        .apply_snapshot(&msg.data[0])
        .expect("snapshot checksum should validate");
    let deltas = parse_public_book_deltas(&msg, &instrument, UnixNanos::from(1_u64))
        .expect("snapshot should parse into deltas");

    assert_eq!(msg.action, "snapshot");
    assert_eq!(state.last_seq(), Some(1001));
    assert_eq!(state.checksum(), msg.data[0].checksum);
    assert_eq!(state.best_bid(), Some(("100.00".to_string(), "1.50".to_string())));
    assert_eq!(state.best_ask(), Some(("100.50".to_string(), "1.00".to_string())));
    assert_eq!(deltas.sequence, 1001);
    assert_eq!(deltas.deltas.len(), 7);
}

#[test]
fn update_mutates_book_state_deterministically() {
    let snapshot = parse_public_book(include_str!("../test_data/ws_public_depth_snapshot.json"))
        .expect("snapshot fixture should deserialize");
    let update = parse_public_book(include_str!("../test_data/ws_public_depth_update.json"))
        .expect("update fixture should deserialize");
    let instrument = make_spot_instrument();
    let mut state = BitgetBookState::default();

    state
        .apply_snapshot(&snapshot.data[0])
        .expect("snapshot checksum should validate");
    state
        .apply_update(&update.data[0])
        .expect("update checksum should validate");

    let deltas = parse_public_book_deltas(&update, &instrument, UnixNanos::from(1_u64))
        .expect("update should parse into deltas");

    assert_eq!(state.last_seq(), Some(1002));
    assert_eq!(state.checksum(), update.data[0].checksum);
    assert_eq!(state.best_bid(), Some(("100.10".to_string(), "1.20".to_string())));
    assert_eq!(state.best_ask(), Some(("100.50".to_string(), "0.80".to_string())));
    assert_eq!(deltas.sequence, 1002);
    assert_eq!(deltas.deltas.len(), 4);
}

#[test]
fn update_requires_snapshot_first() {
    let update = parse_public_book(include_str!("../test_data/ws_public_depth_update.json"))
        .expect("update fixture should deserialize");
    let mut state = BitgetBookState::default();

    let error = state
        .apply_update(&update.data[0])
        .expect_err("update should fail without an initial snapshot");

    assert!(error
        .to_string()
        .contains("update received before initial snapshot"));
}

#[test]
fn trades_parse_into_trade_ticks() {
    let msg = parse_public_trades(include_str!("../test_data/ws_public_trades.json"))
        .expect("trade fixture should deserialize");
    let instrument = make_perp_instrument();

    let first = parse_public_trade_tick(&msg.data[0], &instrument, UnixNanos::from(1_u64))
        .expect("trade should parse");
    let second = parse_public_trade_tick(&msg.data[1], &instrument, UnixNanos::from(1_u64))
        .expect("trade should parse");

    assert_eq!(msg.arg.inst_type, "USDT-FUTURES");
    assert_trade(&first, AggressorSide::Buyer, "100.25", "0.40");
    assert_trade(&second, AggressorSide::Seller, "100.20", "0.10");
}

#[test]
fn ticker_parses_quote_mark_index_and_funding_from_payload() {
    let msg = parse_public_ticker(include_str!("../test_data/ws_public_ticker.json"))
        .expect("ticker fixture should deserialize");
    let instrument = make_spot_instrument();
    let ts_init = UnixNanos::from(1_u64);

    let tick = parse_public_quote_tick(&instrument, &msg.arg, &msg.data[0], ts_init)
        .expect("quote tick should parse");
    let mark = parse_public_mark_price(&instrument, &msg.arg, &msg.data[0], ts_init)
        .expect("mark price should parse");
    let index = parse_public_index_price(&instrument, &msg.arg, &msg.data[0], ts_init)
        .expect("index price should parse");
    let funding = parse_public_funding_rate(&instrument, &msg.arg, &msg.data[0], ts_init)
        .expect("funding rate should parse");

    assert_eq!(msg.action, "update");
    assert_eq!(msg.data[0].inst_id, "BTCUSDT");

    assert_eq!(tick.instrument_id, instrument.id());
    assert_eq!(tick.bid_price, Price::from("100.20"));
    assert_eq!(tick.ask_price, Price::from("100.80"));
    assert_eq!(tick.bid_size, Quantity::from("2.00"));
    assert_eq!(tick.ask_size, Quantity::from("3.00"));

    assert_eq!(mark.instrument_id, instrument.id());
    assert_eq!(mark.value, Price::from("100.60"));
    assert_eq!(index.instrument_id, instrument.id());
    assert_eq!(index.value, Price::from("100.55"));

    assert_eq!(funding.instrument_id, instrument.id());
    assert_eq!(funding.next_funding_ns, Some(UnixNanos::from(1_700_000_005_000_000_000_u64)));
}

#[test]
fn candle_parse_into_bars() {
    let msg = parse_public_candle(include_str!("../test_data/ws_public_candle.json"))
        .expect("candle fixture should deserialize");
    let instrument = make_perp_instrument();

    let bars = parse_public_bars(&msg, &instrument, UnixNanos::from(1_u64))
        .expect("candle fixture should parse");

    assert_eq!(bars.len(), 2);
    let first = bars[0];
    assert_eq!(first.bar_type.aggregation_source(), AggregationSource::External);
    assert_eq!(first.bar_type.instrument_id(), instrument.id());
    assert_eq!(first.open, Price::from("100.00"));
    assert_eq!(first.high, Price::from("101.00"));
    assert_eq!(first.low, Price::from("99.50"));
    assert_eq!(first.close, Price::from("100.75"));
    assert_eq!(first.volume, Quantity::from("10.50"));
    assert_eq!(first.ts_event, UnixNanos::from(1_700_000_000_000_000_000_u64));

    let second = bars[1];
    assert_eq!(second.open, Price::from("100.75"));
    assert_eq!(second.volume, Quantity::from("8.25"));
}

fn assert_trade(
    trade: &TradeTick,
    side: AggressorSide,
    price: &str,
    size: &str,
) {
    assert_eq!(trade.aggressor_side, side);
    assert_eq!(trade.price, Price::from(price));
    assert_eq!(trade.size, Quantity::from(size));
}
