#![no_main]

//! Fuzz WebSocket text frame decode and conversion to Nautilus data.
//!
//! Every decoded decimal must equal its JSON token exactly. Converted prices, sizes, and timestamps
//! must equal their decoded source: conversion rejects finer values rather than rounding them,
//! except for bounded OHLCV volume normalization.

#[path = "../common/mod.rs"]
mod common;

use common::{
    Tokens, assert_exact, assert_level_deltas, assert_levels_exact, assert_volume, instruments,
    nanos,
};
use nautilus_coinapi::{
    http::models::BookLevel,
    websocket::{
        messages::{Book, BookL3Entry, CoinApiWsMessage, DataType, Frame, Ohlcv, Quote, Trade},
        parse::{
            parse_bar, parse_book_deltas, parse_book_depth, parse_quote_tick, parse_trade_tick,
            period_id_to_bar_spec,
        },
    },
};
use nautilus_core::UnixNanos;
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    data::BookOrder,
    enums::{AggressorSide, RecordFlag},
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;

const DEPTH: usize = 20;

fuzz_target!(|data: &[u8]| {
    let Ok(Frame::Data(message)) = Frame::decode(data) else {
        return;
    };

    // Also fails on shapes the payload struct ignores, such as a repeated unknown key
    let Ok(tokens) = serde_json::from_slice::<Tokens>(data) else {
        return;
    };

    match message {
        CoinApiWsMessage::Trade(trade) => check_trade(&trade, &tokens),
        CoinApiWsMessage::Quote(quote) => check_quote(&quote, &tokens),
        CoinApiWsMessage::Ohlcv(ohlcv) => check_ohlcv(&ohlcv, &tokens),
        CoinApiWsMessage::Book(book) => check_book(&book, &tokens),
        CoinApiWsMessage::BookL3(book) => {
            let values = |entry: &BookL3Entry| (entry.price, entry.size);
            assert_levels_exact(
                book.asks.iter().map(values),
                book.bids.iter().map(values),
                &tokens,
            );
        }
        CoinApiWsMessage::Error(_) | CoinApiWsMessage::Reconnected => {
            unreachable!("decode never yields connection events")
        }
    }
});

fn check_trade(trade: &Trade, tokens: &Tokens) {
    assert_exact(tokens.price.unwrap(), trade.price, "price");
    assert_exact(tokens.size.unwrap(), trade.size, "size");

    let aggressor = match trade.taker_side.as_str() {
        "BUY" => AggressorSide::Buy,
        "SELL" => AggressorSide::Sell,
        _ => AggressorSide::NoAggressor,
    };

    for instrument in instruments() {
        let Ok(tick) = parse_trade_tick(trade, &instrument) else {
            continue;
        };

        assert_eq!(tick.price.as_decimal(), trade.price);
        assert_eq!(tick.size.as_decimal(), trade.size);
        assert_eq!(tick.aggressor_side, aggressor);
        assert_eq!(tick.trade_id.as_str(), trade.uuid);
        assert_eq!(Some(tick.ts_event), nanos(trade.time_exchange));
        assert_eq!(Some(tick.ts_init), nanos(trade.time_coinapi));
    }
}

fn check_quote(quote: &Quote, tokens: &Tokens) {
    assert_exact(tokens.bid_price.unwrap(), quote.bid_price, "bid_price");
    assert_exact(tokens.ask_price.unwrap(), quote.ask_price, "ask_price");
    assert_exact(tokens.bid_size.unwrap(), quote.bid_size, "bid_size");
    assert_exact(tokens.ask_size.unwrap(), quote.ask_size, "ask_size");
    for instrument in instruments() {
        let Ok(tick) = parse_quote_tick(quote, &instrument) else {
            continue;
        };

        assert_eq!(tick.bid_price.as_decimal(), quote.bid_price);
        assert_eq!(tick.ask_price.as_decimal(), quote.ask_price);
        assert_eq!(tick.bid_size.as_decimal(), quote.bid_size);
        assert_eq!(tick.ask_size.as_decimal(), quote.ask_size);
        assert_eq!(Some(tick.ts_event), nanos(quote.time_exchange));
        assert_eq!(Some(tick.ts_init), nanos(quote.time_coinapi));
    }
}

fn check_ohlcv(ohlcv: &Ohlcv, tokens: &Tokens) {
    assert_exact(tokens.price_open.unwrap(), ohlcv.price_open, "price_open");
    assert_exact(tokens.price_high.unwrap(), ohlcv.price_high, "price_high");
    assert_exact(tokens.price_low.unwrap(), ohlcv.price_low, "price_low");
    assert_exact(
        tokens.price_close.unwrap(),
        ohlcv.price_close,
        "price_close",
    );
    assert_exact(
        tokens.volume_traded.unwrap(),
        ohlcv.volume_traded,
        "volume_traded",
    );

    let ts_init = UnixNanos::from(1);
    for instrument in instruments() {
        let Ok(bar) = parse_bar(ohlcv, &instrument, ts_init) else {
            continue;
        };

        assert_eq!(
            Some(bar.bar_type.spec()),
            period_id_to_bar_spec(ohlcv.period_id)
        );
        assert_eq!(bar.open.as_decimal(), ohlcv.price_open);
        assert_eq!(bar.high.as_decimal(), ohlcv.price_high);
        assert_eq!(bar.low.as_decimal(), ohlcv.price_low);
        assert_eq!(bar.close.as_decimal(), ohlcv.price_close);
        assert_volume(ohlcv.volume_traded, bar.volume, instrument.size_precision());
        assert_eq!(Some(bar.ts_event), nanos(ohlcv.time_period_end));
        assert_eq!(bar.ts_init, ts_init);
    }
}

fn check_book(book: &Book, tokens: &Tokens) {
    let values = |level: &BookLevel| (level.price, level.size);
    assert_levels_exact(
        book.asks.iter().map(values),
        book.bids.iter().map(values),
        tokens,
    );

    for instrument in instruments() {
        if book.data_type == DataType::Book {
            check_book_deltas(book, &instrument);
        } else {
            check_book_depth(book, &instrument);
        }
    }
}

fn check_book_deltas(book: &Book, instrument: &InstrumentAny) {
    match parse_book_deltas(book, instrument) {
        Ok(Some(deltas)) => assert_level_deltas(book, &deltas, book.sequence),
        Ok(None) => {
            assert_ne!(book.is_snapshot, Some(true));
            assert!(book.bids.is_empty() && book.asks.is_empty());
        }
        Err(_) => {}
    }
}

fn check_book_depth(book: &Book, instrument: &InstrumentAny) {
    let Ok(depth) = parse_book_depth(book, DEPTH, instrument) else {
        return;
    };

    assert_depth_side(&depth.bids, &book.bids);
    assert_depth_side(&depth.asks, &book.asks);
    assert_eq!(depth.flags, RecordFlag::F_SNAPSHOT as u8);
    assert_eq!(depth.sequence, book.sequence);
    assert_eq!(Some(depth.ts_event), nanos(book.time_exchange));
    assert_eq!(Some(depth.ts_init), nanos(book.time_coinapi));
}

// Depth keeps the first `DEPTH` levels and drops any without a positive size
fn assert_depth_side(orders: &[BookOrder], levels: &[BookLevel]) {
    let expected: Vec<(Decimal, Decimal)> = levels
        .iter()
        .take(DEPTH)
        .filter(|level| level.size > Decimal::ZERO)
        .map(|level| (level.price, level.size))
        .collect();
    let actual: Vec<(Decimal, Decimal)> = orders
        .iter()
        .map(|order| (order.price.as_decimal(), order.size.as_decimal()))
        .collect();
    assert_eq!(actual, expected);
}
