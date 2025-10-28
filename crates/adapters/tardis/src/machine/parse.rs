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

use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, Data, FundingRateUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, QuoteTick, TradeTick,
    },
    enums::{AggregationSource, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use uuid::Uuid;

use super::{
    message::{
        BarMsg, BookChangeMsg, BookLevel, BookSnapshotMsg, DerivativeTickerMsg, TradeMsg, WsMessage,
    },
    types::TardisInstrumentMiniInfo,
};
use crate::parse::{normalize_amount, parse_aggressor_side, parse_bar_spec, parse_book_action};

#[must_use]
pub fn parse_tardis_ws_message(
    msg: WsMessage,
    info: Arc<TardisInstrumentMiniInfo>,
) -> Option<Data> {
    match msg {
        WsMessage::BookChange(msg) => {
            if msg.bids.is_empty() && msg.asks.is_empty() {
                tracing::error!(
                    "Invalid book change for {} {} (empty bids and asks)",
                    msg.exchange,
                    msg.symbol
                );
                return None;
            }

            match parse_book_change_msg_as_deltas(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(deltas) => Some(Data::Deltas(deltas)),
                Err(e) => {
                    tracing::error!("Failed to parse book change message: {e}");
                    None
                }
            }
        }
        WsMessage::BookSnapshot(msg) => match msg.bids.len() {
            1 => {
                match parse_book_snapshot_msg_as_quote(
                    msg,
                    info.price_precision,
                    info.size_precision,
                    info.instrument_id,
                ) {
                    Ok(quote) => Some(Data::Quote(quote)),
                    Err(e) => {
                        tracing::error!("Failed to parse book snapshot quote message: {e}");
                        None
                    }
                }
            }
            _ => {
                match parse_book_snapshot_msg_as_deltas(
                    msg,
                    info.price_precision,
                    info.size_precision,
                    info.instrument_id,
                ) {
                    Ok(deltas) => Some(Data::Deltas(deltas)),
                    Err(e) => {
                        tracing::error!("Failed to parse book snapshot message: {e}");
                        None
                    }
                }
            }
        },
        WsMessage::Trade(msg) => {
            match parse_trade_msg(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(trade) => Some(Data::Trade(trade)),
                Err(e) => {
                    tracing::error!("Failed to parse trade message: {e}");
                    None
                }
            }
        }
        WsMessage::TradeBar(msg) => Some(Data::Bar(parse_bar_msg(
            msg,
            info.price_precision,
            info.size_precision,
            info.instrument_id,
        ))),
        // Derivative ticker messages are handled through a separate callback path
        // for FundingRateUpdate since they're not part of the Data enum.
        WsMessage::DerivativeTicker(_) => None,
        WsMessage::Disconnect(_) => None,
    }
}

/// Parse a Tardis WebSocket message specifically for funding rate updates.
/// Returns `Some(FundingRateUpdate)` if the message contains funding rate data, `None` otherwise.
#[must_use]
pub fn parse_tardis_ws_message_funding_rate(
    msg: WsMessage,
    info: Arc<TardisInstrumentMiniInfo>,
) -> Option<FundingRateUpdate> {
    match msg {
        WsMessage::DerivativeTicker(msg) => {
            match parse_derivative_ticker_msg(msg, info.instrument_id) {
                Ok(funding_rate) => funding_rate,
                Err(e) => {
                    tracing::error!(
                        "Failed to parse derivative ticker message for funding rate: {e}"
                    );
                    None
                }
            }
        }
        _ => None, // Only derivative ticker messages can contain funding rates
    }
}

/// Parse a book change message into order book deltas, returning an error if timestamps invalid.
/// Parse a book change message into order book deltas.
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds.
pub fn parse_book_change_msg_as_deltas(
    msg: BookChangeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<OrderBookDeltas_API> {
    parse_book_msg_as_deltas(
        msg.bids,
        msg.asks,
        msg.is_snapshot,
        price_precision,
        size_precision,
        instrument_id,
        msg.timestamp,
        msg.local_timestamp,
    )
}

/// Parse a book snapshot message into order book deltas, returning an error if timestamps invalid.
/// Parse a book snapshot message into order book deltas.
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds.
pub fn parse_book_snapshot_msg_as_deltas(
    msg: BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<OrderBookDeltas_API> {
    parse_book_msg_as_deltas(
        msg.bids,
        msg.asks,
        true,
        price_precision,
        size_precision,
        instrument_id,
        msg.timestamp,
        msg.local_timestamp,
    )
}

/// Parse raw book levels into order book deltas, returning error for invalid timestamps.
#[allow(clippy::too_many_arguments)]
/// Parse raw book levels into order book deltas.
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds.
pub fn parse_book_msg_as_deltas(
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    is_snapshot: bool,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
    timestamp: DateTime<Utc>,
    local_timestamp: DateTime<Utc>,
) -> anyhow::Result<OrderBookDeltas_API> {
    let event_nanos = timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract event nanoseconds")?;
    let ts_event = UnixNanos::from(event_nanos as u64);
    let init_nanos = local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    let ts_init = UnixNanos::from(init_nanos as u64);

    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(bids.len() + asks.len());

    for level in bids {
        deltas.push(parse_book_level(
            instrument_id,
            price_precision,
            size_precision,
            OrderSide::Buy,
            level,
            is_snapshot,
            ts_event,
            ts_init,
        ));
    }

    for level in asks {
        deltas.push(parse_book_level(
            instrument_id,
            price_precision,
            size_precision,
            OrderSide::Sell,
            level,
            is_snapshot,
            ts_event,
            ts_init,
        ));
    }

    if let Some(last_delta) = deltas.last_mut() {
        last_delta.flags += RecordFlag::F_LAST.value();
    }

    // TODO: Opaque pointer wrapper necessary for Cython (remove once Cython gone)
    Ok(OrderBookDeltas_API::new(OrderBookDeltas::new(
        instrument_id,
        deltas,
    )))
}

#[must_use]
/// Parse a single book level into an order book delta.
///
/// # Panics
///
/// Panics if a non-delete action has a zero size after normalization.
#[allow(clippy::too_many_arguments)]
pub fn parse_book_level(
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    side: OrderSide,
    level: BookLevel,
    is_snapshot: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let amount = normalize_amount(level.amount, size_precision);
    let action = parse_book_action(is_snapshot, amount);
    let price = Price::new(level.price, price_precision);
    let size = Quantity::new(amount, size_precision);
    let order_id = 0; // Not applicable for L2 data
    let order = BookOrder::new(side, price, size, order_id);
    let flags = if is_snapshot {
        RecordFlag::F_SNAPSHOT.value()
    } else {
        0
    };
    let sequence = 0; // Not available

    assert!(
        !(action != BookAction::Delete && size.is_zero()),
        "Invalid zero size for {action}"
    );

    OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    )
}

/// Parse a book snapshot message into a quote tick, returning an error on invalid data.
/// Parse a book snapshot message into a quote tick.
///
/// # Errors
///
/// Returns an error if missing bid/ask levels or invalid sizes.
pub fn parse_book_snapshot_msg_as_quote(
    msg: BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<QuoteTick> {
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    let best_bid = msg
        .bids
        .first()
        .context("missing best bid level for quote message")?;
    let bid_price = Price::new(best_bid.price, price_precision);
    let bid_size = Quantity::non_zero_checked(best_bid.amount, size_precision)
        .with_context(|| format!("Invalid bid size for message: {msg:?}"))?;

    let best_ask = msg
        .asks
        .first()
        .context("missing best ask level for quote message")?;
    let ask_price = Price::new(best_ask.price, price_precision);
    let ask_size = Quantity::non_zero_checked(best_ask.amount, size_precision)
        .with_context(|| format!("Invalid ask size for message: {msg:?}"))?;

    Ok(QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

/// Parse a trade message into a trade tick, returning an error on invalid data.
/// Parse a trade message into a trade tick.
///
/// # Errors
///
/// Returns an error if invalid trade size is encountered.
pub fn parse_trade_msg(
    msg: TradeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<TradeTick> {
    let price = Price::new(msg.price, price_precision);
    let size = Quantity::non_zero_checked(msg.amount, size_precision)
        .with_context(|| format!("Invalid trade size in message: {msg:?}"))?;
    let aggressor_side = parse_aggressor_side(&msg.side);
    let trade_id = TradeId::new(msg.id.unwrap_or_else(|| Uuid::new_v4().to_string()));
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

#[must_use]
pub fn parse_bar_msg(
    msg: BarMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> Bar {
    let spec = parse_bar_spec(&msg.name);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);
    let volume = Quantity::non_zero(msg.volume, size_precision);
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parse a derivative ticker message into a funding rate update.
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds or decimal conversion fails.
pub fn parse_derivative_ticker_msg(
    msg: DerivativeTickerMsg,
    instrument_id: InstrumentId,
) -> anyhow::Result<Option<FundingRateUpdate>> {
    // Only process if we have funding rate data
    let funding_rate = match msg.funding_rate {
        Some(rate) => rate,
        None => return Ok(None), // No funding rate data
    };

    let ts_event = msg
        .timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract event nanoseconds")?;
    let ts_event = UnixNanos::from(ts_event as u64);

    let ts_init = msg
        .local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    let ts_init = UnixNanos::from(ts_init as u64);

    let rate = rust_decimal::Decimal::try_from(funding_rate)
        .with_context(|| format!("Failed to convert funding rate {funding_rate} to Decimal"))?
        .normalize();

    // For live data, we don't typically have funding timestamp info from derivative ticker
    let next_funding_ns = None;

    Ok(Some(FundingRateUpdate::new(
        instrument_id,
        rate,
        next_funding_ns,
        ts_event,
        ts_init,
    )))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::enums::AggressorSide;
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    #[rstest]
    fn test_parse_book_change_message() {
        let json_data = load_test_json("book_change.json");
        let msg: BookChangeMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 0;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let deltas =
            parse_book_change_msg_as_deltas(msg, price_precision, size_precision, instrument_id)
                .unwrap();

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.flags, RecordFlag::F_LAST.value());
        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(1571830193469000000));
        assert_eq!(deltas.ts_init, UnixNanos::from(1571830193469000000));
        assert_eq!(
            deltas.deltas[0].instrument_id,
            InstrumentId::from("XBTUSD.BITMEX")
        );
        assert_eq!(deltas.deltas[0].action, BookAction::Update);
        assert_eq!(deltas.deltas[0].order.price, Price::from("7985"));
        assert_eq!(deltas.deltas[0].order.size, Quantity::from(283318));
        assert_eq!(deltas.deltas[0].order.order_id, 0);
        assert_eq!(deltas.deltas[0].flags, RecordFlag::F_LAST.value());
        assert_eq!(deltas.deltas[0].sequence, 0);
        assert_eq!(
            deltas.deltas[0].ts_event,
            UnixNanos::from(1571830193469000000)
        );
        assert_eq!(
            deltas.deltas[0].ts_init,
            UnixNanos::from(1571830193469000000)
        );
    }

    #[rstest]
    fn test_parse_book_snapshot_message_as_deltas() {
        let json_data = load_test_json("book_snapshot.json");
        let msg: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 1;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let deltas =
            parse_book_snapshot_msg_as_deltas(msg, price_precision, size_precision, instrument_id)
                .unwrap();
        let delta_0 = deltas.deltas[0];
        let delta_2 = deltas.deltas[2];

        assert_eq!(deltas.deltas.len(), 4);
        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(
            deltas.flags,
            RecordFlag::F_LAST.value() + RecordFlag::F_SNAPSHOT.value()
        );
        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(deltas.ts_init, UnixNanos::from(1572010786961000000));
        assert_eq!(delta_0.instrument_id, instrument_id);
        assert_eq!(delta_0.action, BookAction::Add);
        assert_eq!(delta_0.order.side, OrderSide::Buy);
        assert_eq!(delta_0.order.price, Price::from("7633.5"));
        assert_eq!(delta_0.order.size, Quantity::from(1906067));
        assert_eq!(delta_0.order.order_id, 0);
        assert_eq!(delta_0.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(delta_0.sequence, 0);
        assert_eq!(delta_0.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(delta_0.ts_init, UnixNanos::from(1572010786961000000));
        assert_eq!(delta_2.instrument_id, instrument_id);
        assert_eq!(delta_2.action, BookAction::Add);
        assert_eq!(delta_2.order.side, OrderSide::Sell);
        assert_eq!(delta_2.order.price, Price::from("7634.0"));
        assert_eq!(delta_2.order.size, Quantity::from(1467849));
        assert_eq!(delta_2.order.order_id, 0);
        assert_eq!(delta_2.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(delta_2.sequence, 0);
        assert_eq!(delta_2.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(delta_2.ts_init, UnixNanos::from(1572010786961000000));
    }

    #[rstest]
    fn test_parse_book_snapshot_message_as_quote() {
        let json_data = load_test_json("book_snapshot.json");
        let msg: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 1;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let quote =
            parse_book_snapshot_msg_as_quote(msg, price_precision, size_precision, instrument_id)
                .expect("Failed to parse book snapshot quote message");

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("7633.5"));
        assert_eq!(quote.bid_size, Quantity::from(1906067));
        assert_eq!(quote.ask_price, Price::from("7634.0"));
        assert_eq!(quote.ask_size, Quantity::from(1467849));
        assert_eq!(quote.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(quote.ts_init, UnixNanos::from(1572010786961000000));
    }

    #[rstest]
    fn test_parse_trade_message() {
        let json_data = load_test_json("trade.json");
        let msg: TradeMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 0;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let trade = parse_trade_msg(msg, price_precision, size_precision, instrument_id)
            .expect("Failed to parse trade message");

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("7996"));
        assert_eq!(trade.size, Quantity::from(50));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.ts_event, UnixNanos::from(1571826769669000000));
        assert_eq!(trade.ts_init, UnixNanos::from(1571826769740000000));
    }

    #[rstest]
    fn test_parse_bar_message() {
        let json_data = load_test_json("bar.json");
        let msg: BarMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 1;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let bar = parse_bar_msg(msg, price_precision, size_precision, instrument_id);

        assert_eq!(
            bar.bar_type,
            BarType::from("XBTUSD.BITMEX-10000-MILLISECOND-LAST-EXTERNAL")
        );
        assert_eq!(bar.open, Price::from("7623.5"));
        assert_eq!(bar.high, Price::from("7623.5"));
        assert_eq!(bar.low, Price::from("7623"));
        assert_eq!(bar.close, Price::from("7623.5"));
        assert_eq!(bar.volume, Quantity::from(37034));
        assert_eq!(bar.ts_event, UnixNanos::from(1572009100000000000));
        assert_eq!(bar.ts_init, UnixNanos::from(1572009100369000000));
    }
}
