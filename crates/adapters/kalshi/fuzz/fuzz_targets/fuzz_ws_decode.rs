#![no_main]

use nautilus_kalshi::{
    common::parse::{parse_instrument_id, parse_ticker},
    data::{quote, trade},
    websocket::{messages::KalshiWsMessage, parse::decode_frame},
};
use nautilus_live::fuzz::fuzz_target;
use nautilus_model::instruments::BinaryOption;
use rust_decimal::Decimal;

#[path = "../common/mod.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Some(data) = common::capped(data) else {
        return;
    };
    let instrument = common::instrument(None);
    probe(&instrument, data);
    if let Ok(text) = std::str::from_utf8(data) {
        probe_text(&instrument, text);
    }
});

fn probe(instrument: &BinaryOption, data: &[u8]) {
    let first = decode_frame(data);
    let second = decode_frame(data);
    assert_eq!(first.is_ok(), second.is_ok(), "frame rejection changed");

    let Ok(message) = first else {
        return;
    };

    assert_eq!(message, second.unwrap(), "decoded frame changed");
    assert_domain(instrument, &message);
}

fn probe_text(instrument: &BinaryOption, text: &str) {
    probe(instrument, text.as_bytes());

    for kind in ["ticker", "trade", "orderbook_snapshot", "orderbook_delta"] {
        let frame = format!(r#"{{"type":"{kind}","sid":1,"seq":1,"msg":{text}}}"#);
        probe(instrument, frame.as_bytes());
    }
}

fn assert_domain(instrument: &BinaryOption, message: &KalshiWsMessage) {
    match message {
        KalshiWsMessage::Ticker(data) => {
            if let Ok(Some(tick)) = quote(
                data.msg.clone(),
                instrument,
                nautilus_core::UnixNanos::from(1),
            ) {
                assert_price(tick.bid_price.as_decimal());
                assert_price(tick.ask_price.as_decimal());
                assert!(tick.bid_price <= tick.ask_price, "decoded quote is crossed");
                assert!(tick.bid_size.is_positive() && tick.ask_size.is_positive());
            }
        }
        KalshiWsMessage::Trade(data) => {
            if let Ok(Some(tick)) = trade(
                data.msg.clone(),
                instrument,
                nautilus_core::UnixNanos::from(1),
            ) {
                assert_price(tick.price.as_decimal());
                assert!(
                    tick.size.is_positive(),
                    "decoded trade size is not positive"
                );
            }
        }
        other => {
            if let Some((_, data)) = other.data()
                && let Some(ticker) = data.msg.get("market_ticker").and_then(|v| v.as_str())
                && let Ok(id) = parse_instrument_id(ticker)
            {
                assert_eq!(parse_ticker(id).unwrap(), ticker);
            }
        }
    }
}

fn assert_price(price: Decimal) {
    assert!(
        price >= Decimal::ZERO && price <= Decimal::ONE,
        "decoded price left the binary payout interval"
    );
}
