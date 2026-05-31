#![no_main]

use libfuzzer_sys::fuzz_target;
use nautilus_derive::websocket::{
    DeriveOrdersSubscriptionData, DeriveTradesSubscriptionData, DeriveWsFrame, parse_public_ws_data,
};

const CHANNELS: [&str; 7] = [
    "orderbook.ETH-PERP.1.10",
    "trades.perp.ETH",
    "ticker_slim.ETH-PERP.1000",
    "ticker_slim.ETH-20260627-3500-C.1000",
    "30769.orders",
    "30769.trades",
    "30769.balances",
];

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        decode_text(text);
    }

    if data.len() > 1
        && let Ok(text) = std::str::from_utf8(&data[1..])
    {
        decode_text(text);
    }
});

fn decode_text(text: &str) {
    decode_frame(text);

    if serde_json::from_str::<serde_json::Value>(text).is_err() {
        return;
    }

    for channel in CHANNELS {
        let frame = format!(
            r#"{{"jsonrpc":"2.0","method":"subscription","params":{{"channel":"{channel}","data":{text}}}}}"#
        );
        decode_frame(&frame);
    }
}

fn decode_frame(text: &str) {
    let Ok(frame) = DeriveWsFrame::parse(text) else {
        return;
    };

    let DeriveWsFrame::Subscription(payload) = frame else {
        return;
    };

    let channel = payload.channel.as_str();
    if channel.starts_with("orderbook.")
        || channel.starts_with("trades.")
        || channel.starts_with("ticker_slim.")
        || channel.starts_with("ticker.")
    {
        let _ = parse_public_ws_data(&payload);
    } else if channel.ends_with(".orders") {
        let _ = serde_json::from_str::<DeriveOrdersSubscriptionData>(payload.data.get());
    } else if channel.ends_with(".trades") {
        let _ = serde_json::from_str::<DeriveTradesSubscriptionData>(payload.data.get());
    }
}
