#![no_main]

//! Fuzz REST history and order book response decode and conversion.
//!
//! Decodes the input as each response shape. Every decoded decimal must equal its JSON token
//! exactly, and every converted quote, trade, and bar must carry its source values and
//! `time_exchange` without rounding, except for bounded OHLCV volume normalization. An order book
//! snapshot whose levels all have positive sizes must report the best bid and ask of its
//! listed levels.

#[path = "../common/mod.rs"]
mod common;

use common::{Tokens, assert_exact, assert_levels_exact, assert_volume, instruments, nanos};
use nautilus_coinapi::{
    http::{
        models::{Book, BookLevel, Ohlcv, Quote, Trade},
        parse::{parse_bar, parse_order_book, parse_quote_tick, parse_trade_tick},
    },
    websocket::parse::parse_aggressor_side,
};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::{
    data::BarType,
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;

fuzz_target!(|data: &[u8]| {
    let instruments = instruments();

    if let Ok(quotes) = serde_json::from_slice::<Vec<Quote>>(data)
        && let Ok(tokens) = serde_json::from_slice::<Vec<Tokens>>(data)
    {
        for (quote, tokens) in quotes.iter().zip(&tokens) {
            check_quote(quote, tokens, &instruments);
        }
    }

    if let Ok(trades) = serde_json::from_slice::<Vec<Trade>>(data)
        && let Ok(tokens) = serde_json::from_slice::<Vec<Tokens>>(data)
    {
        for (trade, tokens) in trades.iter().zip(&tokens) {
            check_trade(trade, tokens, &instruments);
        }
    }

    if let Ok(bars) = serde_json::from_slice::<Vec<Ohlcv>>(data)
        && let Ok(tokens) = serde_json::from_slice::<Vec<Tokens>>(data)
    {
        for (ohlcv, tokens) in bars.iter().zip(&tokens) {
            check_ohlcv(ohlcv, tokens, &instruments);
        }
    }

    if let Ok(book) = serde_json::from_slice::<Book>(data)
        && let Ok(tokens) = serde_json::from_slice::<Tokens>(data)
    {
        check_book(&book, &tokens, &instruments);
    }
});

fn check_quote(quote: &Quote, tokens: &Tokens, instruments: &[InstrumentAny]) {
    assert_exact(tokens.bid_price.unwrap(), quote.bid_price, "bid_price");
    assert_exact(tokens.ask_price.unwrap(), quote.ask_price, "ask_price");
    assert_exact(tokens.bid_size.unwrap(), quote.bid_size, "bid_size");
    assert_exact(tokens.ask_size.unwrap(), quote.ask_size, "ask_size");
    for instrument in instruments {
        let Ok(tick) = parse_quote_tick(quote, instrument) else {
            continue;
        };

        assert_eq!(tick.bid_price.as_decimal(), quote.bid_price);
        assert_eq!(tick.ask_price.as_decimal(), quote.ask_price);
        assert_eq!(tick.bid_size.as_decimal(), quote.bid_size);
        assert_eq!(tick.ask_size.as_decimal(), quote.ask_size);
        assert_eq!(Some(tick.ts_event), nanos(quote.time_exchange));
        assert_eq!(tick.ts_init, tick.ts_event);
    }
}

fn check_trade(trade: &Trade, tokens: &Tokens, instruments: &[InstrumentAny]) {
    assert_exact(tokens.price.unwrap(), trade.price, "price");
    assert_exact(tokens.size.unwrap(), trade.size, "size");
    for instrument in instruments {
        let Ok(tick) = parse_trade_tick(trade, instrument) else {
            continue;
        };

        assert_eq!(tick.price.as_decimal(), trade.price);
        assert_eq!(tick.size.as_decimal(), trade.size);
        assert_eq!(tick.aggressor_side, parse_aggressor_side(trade.taker_side));
        assert_eq!(tick.trade_id.as_str(), trade.uuid);
        assert_eq!(Some(tick.ts_event), nanos(trade.time_exchange));
        assert_eq!(tick.ts_init, tick.ts_event);
    }
}

fn check_ohlcv(ohlcv: &Ohlcv, tokens: &Tokens, instruments: &[InstrumentAny]) {
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

    for instrument in instruments {
        let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id()));

        let Ok(bar) = parse_bar(ohlcv, bar_type, instrument) else {
            continue;
        };

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open.as_decimal(), ohlcv.price_open);
        assert_eq!(bar.high.as_decimal(), ohlcv.price_high);
        assert_eq!(bar.low.as_decimal(), ohlcv.price_low);
        assert_eq!(bar.close.as_decimal(), ohlcv.price_close);
        assert_volume(ohlcv.volume_traded, bar.volume, instrument.size_precision());
        assert_eq!(Some(bar.ts_event), nanos(ohlcv.time_period_end));
        assert_eq!(bar.ts_init, bar.ts_event);
    }
}

fn check_book(book: &Book, tokens: &Tokens, instruments: &[InstrumentAny]) {
    let values = |level: &BookLevel| (level.price, level.size);
    assert_levels_exact(
        book.asks.iter().map(values),
        book.bids.iter().map(values),
        tokens,
    );

    for instrument in instruments {
        let Ok(order_book) = parse_order_book(book, instrument) else {
            continue;
        };

        assert_eq!(order_book.instrument_id, instrument.id());

        // Resting levels keep positive sizes, so the best prices bound the listed ones
        if book
            .bids
            .iter()
            .chain(&book.asks)
            .all(|level| level.size > Decimal::ZERO)
        {
            let best_bid = book.bids.iter().map(|level| level.price).max();
            let best_ask = book.asks.iter().map(|level| level.price).min();
            assert_eq!(
                order_book.best_bid_price().map(|price| price.as_decimal()),
                best_bid
            );
            assert_eq!(
                order_book.best_ask_price().map(|price| price.as_decimal()),
                best_ask
            );
        }
    }
}
