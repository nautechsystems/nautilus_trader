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
        Bar, BarType, BookOrder, DEPTH10_LEN, Data, FundingRateUpdate, NULL_ORDER, OrderBookDelta,
        OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
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
use crate::{
    config::BookSnapshotOutput,
    parse::{normalize_amount, parse_aggressor_side, parse_bar_spec, parse_book_action},
};

#[must_use]
pub fn parse_tardis_ws_message(
    msg: WsMessage,
    info: Arc<TardisInstrumentMiniInfo>,
    book_snapshot_output: &BookSnapshotOutput,
) -> Option<Data> {
    match msg {
        WsMessage::BookChange(msg) => {
            if msg.bids.is_empty() && msg.asks.is_empty() {
                let exchange = msg.exchange;
                let symbol = &msg.symbol;
                tracing::error!(
                    "Invalid book change for {exchange} {symbol} (empty bids and asks)"
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
            _ => match book_snapshot_output {
                BookSnapshotOutput::Depth10 => {
                    match parse_book_snapshot_msg_as_depth10(
                        msg,
                        info.price_precision,
                        info.size_precision,
                        info.instrument_id,
                    ) {
                        Ok(depth10) => Some(Data::Depth10(Box::new(depth10))),
                        Err(e) => {
                            tracing::error!("Failed to parse book snapshot as depth10: {e}");
                            None
                        }
                    }
                }
                BookSnapshotOutput::Deltas => {
                    match parse_book_snapshot_msg_as_deltas(
                        msg,
                        info.price_precision,
                        info.size_precision,
                        info.instrument_id,
                    ) {
                        Ok(deltas) => Some(Data::Deltas(deltas)),
                        Err(e) => {
                            tracing::error!("Failed to parse book snapshot as deltas: {e}");
                            None
                        }
                    }
                }
            },
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
        WsMessage::TradeBar(msg) => {
            match parse_bar_msg(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(bar) => Some(Data::Bar(bar)),
                Err(e) => {
                    tracing::error!("Failed to parse bar message: {e}");
                    None
                }
            }
        }
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

/// Parse a book snapshot message into an [`OrderBookDepth10`].
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds.
pub fn parse_book_snapshot_msg_as_depth10(
    msg: BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<OrderBookDepth10> {
    let ts_event_nanos = msg
        .timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract event nanoseconds")?;
    anyhow::ensure!(
        ts_event_nanos >= 0,
        "invalid timestamp: event nanoseconds {ts_event_nanos} is before UNIX epoch"
    );
    let ts_event = UnixNanos::from(ts_event_nanos as u64);

    let ts_init_nanos = msg
        .local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    anyhow::ensure!(
        ts_init_nanos >= 0,
        "invalid timestamp: init nanoseconds {ts_init_nanos} is before UNIX epoch"
    );
    let ts_init = UnixNanos::from(ts_init_nanos as u64);

    let mut bids = [NULL_ORDER; DEPTH10_LEN];
    let mut asks = [NULL_ORDER; DEPTH10_LEN];
    let mut bid_counts = [0u32; DEPTH10_LEN];
    let mut ask_counts = [0u32; DEPTH10_LEN];

    for (i, level) in msg.bids.iter().take(DEPTH10_LEN).enumerate() {
        bids[i] = BookOrder::new(
            OrderSide::Buy,
            Price::new(level.price, price_precision),
            Quantity::new(level.amount, size_precision),
            0,
        );
        bid_counts[i] = 1;
    }

    for (i, level) in msg.asks.iter().take(DEPTH10_LEN).enumerate() {
        asks[i] = BookOrder::new(
            OrderSide::Sell,
            Price::new(level.price, price_precision),
            Quantity::new(level.amount, size_precision),
            0,
        );
        ask_counts[i] = 1;
    }

    Ok(OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT.value(),
        0, // Sequence not available from Tardis
        ts_event,
        ts_init,
    ))
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
    anyhow::ensure!(
        event_nanos >= 0,
        "invalid timestamp: event nanoseconds {event_nanos} is before UNIX epoch"
    );
    let ts_event = UnixNanos::from(event_nanos as u64);
    let init_nanos = local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    anyhow::ensure!(
        init_nanos >= 0,
        "invalid timestamp: init nanoseconds {init_nanos} is before UNIX epoch"
    );
    let ts_init = UnixNanos::from(init_nanos as u64);

    let capacity = if is_snapshot {
        bids.len() + asks.len() + 1
    } else {
        bids.len() + asks.len()
    };
    let mut deltas: Vec<OrderBookDelta> = Vec::with_capacity(capacity);

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init));
    }

    for level in bids {
        match parse_book_level(
            instrument_id,
            price_precision,
            size_precision,
            OrderSide::Buy,
            level,
            is_snapshot,
            ts_event,
            ts_init,
        ) {
            Ok(delta) => deltas.push(delta),
            Err(e) => tracing::warn!("Skipping invalid bid level for {instrument_id}: {e}"),
        }
    }

    for level in asks {
        match parse_book_level(
            instrument_id,
            price_precision,
            size_precision,
            OrderSide::Sell,
            level,
            is_snapshot,
            ts_event,
            ts_init,
        ) {
            Ok(delta) => deltas.push(delta),
            Err(e) => tracing::warn!("Skipping invalid ask level for {instrument_id}: {e}"),
        }
    }

    if let Some(last_delta) = deltas.last_mut() {
        last_delta.flags |= RecordFlag::F_LAST.value();
    }

    // TODO: Opaque pointer wrapper necessary for Cython (remove once Cython gone)
    Ok(OrderBookDeltas_API::new(OrderBookDeltas::new(
        instrument_id,
        deltas,
    )))
}

/// Parse a single book level into an order book delta.
///
/// # Errors
///
/// Returns an error if a non-delete action has a zero size after normalization.
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
) -> anyhow::Result<OrderBookDelta> {
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

    anyhow::ensure!(
        !(action != BookAction::Delete && size.is_zero()),
        "Invalid zero size for {action}"
    );

    Ok(OrderBookDelta::new(
        instrument_id,
        action,
        order,
        flags,
        sequence,
        ts_event,
        ts_init,
    ))
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

/// Parse a bar message into a Bar.
///
/// # Errors
///
/// Returns an error if the bar specification cannot be parsed.
pub fn parse_bar_msg(
    msg: BarMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<Bar> {
    let spec = parse_bar_spec(&msg.name)?;
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);
    let volume = Quantity::non_zero(msg.volume, size_precision);
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    Ok(Bar::new(
        bar_type, open, high, low, close, volume, ts_event, ts_init,
    ))
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

    let ts_event_nanos = msg
        .timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract event nanoseconds")?;
    anyhow::ensure!(
        ts_event_nanos >= 0,
        "invalid timestamp: event nanoseconds {ts_event_nanos} is before UNIX epoch"
    );
    let ts_event = UnixNanos::from(ts_event_nanos as u64);

    let ts_init_nanos = msg
        .local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    anyhow::ensure!(
        ts_init_nanos >= 0,
        "invalid timestamp: init nanoseconds {ts_init_nanos} is before UNIX epoch"
    );
    let ts_init = UnixNanos::from(ts_init_nanos as u64);

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

#[cfg(test)]
mod tests {
    use nautilus_model::enums::AggressorSide;
    use rstest::rstest;

    use super::*;
    use crate::{enums::TardisExchange, tests::load_test_json};

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

        let clear_delta = deltas.deltas[0];
        let bid_delta = deltas.deltas[1];
        let ask_delta = deltas.deltas[3];

        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(
            deltas.flags,
            RecordFlag::F_LAST.value() + RecordFlag::F_SNAPSHOT.value()
        );
        assert_eq!(deltas.sequence, 0);
        assert_eq!(deltas.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(deltas.ts_init, UnixNanos::from(1572010786961000000));

        // CLEAR delta
        assert_eq!(clear_delta.instrument_id, instrument_id);
        assert_eq!(clear_delta.action, BookAction::Clear);
        assert_eq!(clear_delta.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(clear_delta.sequence, 0);
        assert_eq!(clear_delta.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(clear_delta.ts_init, UnixNanos::from(1572010786961000000));

        // First bid delta
        assert_eq!(bid_delta.instrument_id, instrument_id);
        assert_eq!(bid_delta.action, BookAction::Add);
        assert_eq!(bid_delta.order.side, OrderSide::Buy);
        assert_eq!(bid_delta.order.price, Price::from("7633.5"));
        assert_eq!(bid_delta.order.size, Quantity::from(1906067));
        assert_eq!(bid_delta.order.order_id, 0);
        assert_eq!(bid_delta.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(bid_delta.sequence, 0);
        assert_eq!(bid_delta.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(bid_delta.ts_init, UnixNanos::from(1572010786961000000));

        // First ask delta
        assert_eq!(ask_delta.instrument_id, instrument_id);
        assert_eq!(ask_delta.action, BookAction::Add);
        assert_eq!(ask_delta.order.side, OrderSide::Sell);
        assert_eq!(ask_delta.order.price, Price::from("7634.0"));
        assert_eq!(ask_delta.order.size, Quantity::from(1467849));
        assert_eq!(ask_delta.order.order_id, 0);
        assert_eq!(ask_delta.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(ask_delta.sequence, 0);
        assert_eq!(ask_delta.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(ask_delta.ts_init, UnixNanos::from(1572010786961000000));
    }

    #[rstest]
    fn test_parse_book_snapshot_message_as_depth10() {
        let json_data = load_test_json("book_snapshot.json");
        let msg: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 1;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

        let depth10 =
            parse_book_snapshot_msg_as_depth10(msg, price_precision, size_precision, instrument_id)
                .unwrap();

        assert_eq!(depth10.instrument_id, instrument_id);
        assert_eq!(depth10.flags, RecordFlag::F_SNAPSHOT.value());
        assert_eq!(depth10.sequence, 0);
        assert_eq!(depth10.ts_event, UnixNanos::from(1572010786950000000));
        assert_eq!(depth10.ts_init, UnixNanos::from(1572010786961000000));

        // Check first bid level
        assert_eq!(depth10.bids[0].side, OrderSide::Buy);
        assert_eq!(depth10.bids[0].price, Price::from("7633.5"));
        assert_eq!(depth10.bids[0].size, Quantity::from(1906067));
        assert_eq!(depth10.bids[0].order_id, 0);
        assert_eq!(depth10.bid_counts[0], 1);

        // Check second bid level
        assert_eq!(depth10.bids[1].side, OrderSide::Buy);
        assert_eq!(depth10.bids[1].price, Price::from("7633.0"));
        assert_eq!(depth10.bids[1].size, Quantity::from(65319));
        assert_eq!(depth10.bid_counts[1], 1);

        // Check first ask level
        assert_eq!(depth10.asks[0].side, OrderSide::Sell);
        assert_eq!(depth10.asks[0].price, Price::from("7634.0"));
        assert_eq!(depth10.asks[0].size, Quantity::from(1467849));
        assert_eq!(depth10.asks[0].order_id, 0);
        assert_eq!(depth10.ask_counts[0], 1);

        // Check second ask level
        assert_eq!(depth10.asks[1].side, OrderSide::Sell);
        assert_eq!(depth10.asks[1].price, Price::from("7634.5"));
        assert_eq!(depth10.asks[1].size, Quantity::from(67939));
        assert_eq!(depth10.ask_counts[1], 1);

        // Check empty levels are NULL_ORDER
        assert_eq!(depth10.bids[2], NULL_ORDER);
        assert_eq!(depth10.bid_counts[2], 0);
        assert_eq!(depth10.asks[2], NULL_ORDER);
        assert_eq!(depth10.ask_counts[2], 0);
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
        let bar = parse_bar_msg(msg, price_precision, size_precision, instrument_id).unwrap();

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

    #[rstest]
    fn test_parse_tardis_ws_message_book_snapshot_routes_to_depth10() {
        let json_data = load_test_json("book_snapshot.json");
        let msg: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();
        let ws_msg = WsMessage::BookSnapshot(msg);

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let info = Arc::new(TardisInstrumentMiniInfo::new(
            instrument_id,
            None,
            TardisExchange::Bitmex,
            1,
            0,
        ));

        let result = parse_tardis_ws_message(ws_msg, info, &BookSnapshotOutput::Depth10);

        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Data::Depth10(_)));
    }

    #[rstest]
    fn test_parse_tardis_ws_message_book_snapshot_routes_to_deltas() {
        let json_data = load_test_json("book_snapshot.json");
        let msg: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();
        let ws_msg = WsMessage::BookSnapshot(msg);

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let info = Arc::new(TardisInstrumentMiniInfo::new(
            instrument_id,
            None,
            TardisExchange::Bitmex,
            1,
            0,
        ));

        let result = parse_tardis_ws_message(ws_msg, info, &BookSnapshotOutput::Deltas);

        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Data::Deltas(_)));
    }
}
