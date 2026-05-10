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

use std::sync::Arc;

use anyhow::Context;
use chrono::{DateTime, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, DEPTH10_LEN, Data, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, NULL_ORDER, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AggregationSource, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

use super::{
    message::{
        BarMsg, BookChangeMsg, BookLevel, BookSnapshotMsg, DerivativeTickerMsg, TradeMsg, WsMessage,
    },
    types::TardisInstrumentMiniInfo,
};
use crate::{
    common::parse::{
        derive_trade_id, normalize_amount, parse_aggressor_side, parse_bar_spec, parse_book_action,
    },
    config::BookSnapshotOutput,
};

#[must_use]
pub fn parse_tardis_ws_message(
    msg: WsMessage,
    info: &Arc<TardisInstrumentMiniInfo>,
    book_snapshot_output: &BookSnapshotOutput,
) -> Option<Data> {
    match msg {
        WsMessage::BookChange(msg) => {
            if msg.bids.is_empty() && msg.asks.is_empty() {
                let exchange = msg.exchange;
                let symbol = &msg.symbol;
                log::error!("Invalid book change for {exchange} {symbol} (empty bids and asks)");
                return None;
            }

            match parse_book_change_msg_as_deltas(
                &msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(deltas) => Some(Data::Deltas(deltas)),
                Err(e) => {
                    log::error!("Failed to parse book change message: {e}");
                    None
                }
            }
        }
        WsMessage::BookSnapshot(msg) => match msg.depth {
            1 => {
                match parse_book_snapshot_msg_as_quote(
                    &msg,
                    info.price_precision,
                    info.size_precision,
                    info.instrument_id,
                ) {
                    Ok(quote) => Some(Data::Quote(quote)),
                    Err(e) => {
                        log::error!("Failed to parse book snapshot quote message: {e}");
                        None
                    }
                }
            }
            _ => match book_snapshot_output {
                BookSnapshotOutput::Depth10 => {
                    match parse_book_snapshot_msg_as_depth10(
                        &msg,
                        info.price_precision,
                        info.size_precision,
                        info.instrument_id,
                    ) {
                        Ok(depth10) => Some(Data::Depth10(Box::new(depth10))),
                        Err(e) => {
                            log::error!("Failed to parse book snapshot as depth10: {e}");
                            None
                        }
                    }
                }
                BookSnapshotOutput::Deltas => {
                    match parse_book_snapshot_msg_as_deltas(
                        &msg,
                        info.price_precision,
                        info.size_precision,
                        info.instrument_id,
                    ) {
                        Ok(deltas) => Some(Data::Deltas(deltas)),
                        Err(e) => {
                            log::error!("Failed to parse book snapshot as deltas: {e}");
                            None
                        }
                    }
                }
            },
        },
        WsMessage::Trade(msg) => {
            match parse_trade_msg(
                &msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(trade) => Some(Data::Trade(trade)),
                Err(e) => {
                    log::error!("Failed to parse trade message: {e}");
                    None
                }
            }
        }
        WsMessage::TradeBar(msg) => {
            match parse_bar_msg(
                &msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ) {
                Ok(bar) => Some(Data::Bar(bar)),
                Err(e) => {
                    log::error!("Failed to parse bar message: {e}");
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
    info: &Arc<TardisInstrumentMiniInfo>,
) -> Option<FundingRateUpdate> {
    match msg {
        WsMessage::DerivativeTicker(msg) => {
            match parse_derivative_ticker_msg(&msg, info.instrument_id) {
                Ok(funding_rate) => funding_rate,
                Err(e) => {
                    log::error!("Failed to parse derivative ticker message for funding rate: {e}");
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
    msg: &BookChangeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<OrderBookDeltas_API> {
    parse_book_msg_as_deltas(
        &msg.bids,
        &msg.asks,
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
    msg: &BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<OrderBookDeltas_API> {
    parse_book_msg_as_deltas(
        &msg.bids,
        &msg.asks,
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
    msg: &BookSnapshotMsg,
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
#[expect(clippy::too_many_arguments)]
/// Parse raw book levels into order book deltas.
///
/// # Errors
///
/// Returns an error if timestamp fields cannot be converted to nanoseconds.
pub fn parse_book_msg_as_deltas(
    bids: &[BookLevel],
    asks: &[BookLevel],
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
            Err(e) => log::warn!("Skipping invalid bid level for {instrument_id}: {e}"),
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
            Err(e) => log::warn!("Skipping invalid ask level for {instrument_id}: {e}"),
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
#[expect(clippy::too_many_arguments)]
pub fn parse_book_level(
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    side: OrderSide,
    level: &BookLevel,
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
    msg: &BookSnapshotMsg,
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
    msg: &TradeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> anyhow::Result<TradeTick> {
    let price = Price::new(msg.price, price_precision);
    let size = Quantity::non_zero_checked(msg.amount, size_precision)
        .with_context(|| format!("Invalid trade size in message: {msg:?}"))?;
    let aggressor_side = parse_aggressor_side(&msg.side);
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);
    let trade_id = match msg.id.as_deref() {
        Some(id) if !id.is_empty() => TradeId::new(id),
        _ => derive_trade_id(
            msg.symbol,
            ts_event.as_u64(),
            msg.price,
            msg.amount,
            &msg.side,
        ),
    };

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
    msg: &BarMsg,
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

/// Extracts event and init timestamps from a derivative ticker message.
fn parse_derivative_ticker_timestamps(
    msg: &DerivativeTickerMsg,
) -> anyhow::Result<(UnixNanos, UnixNanos)> {
    let ts_event_nanos = msg
        .timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract event nanoseconds")?;
    anyhow::ensure!(
        ts_event_nanos >= 0,
        "invalid timestamp: event nanoseconds {ts_event_nanos} is before UNIX epoch"
    );

    let ts_init_nanos = msg
        .local_timestamp
        .timestamp_nanos_opt()
        .context("invalid timestamp: cannot extract init nanoseconds")?;
    anyhow::ensure!(
        ts_init_nanos >= 0,
        "invalid timestamp: init nanoseconds {ts_init_nanos} is before UNIX epoch"
    );

    Ok((
        UnixNanos::from(ts_event_nanos as u64),
        UnixNanos::from(ts_init_nanos as u64),
    ))
}

/// Parses a derivative ticker message into a funding rate update.
///
/// # Errors
///
/// Returns an error if timestamp conversion or decimal conversion fails.
pub fn parse_derivative_ticker_msg(
    msg: &DerivativeTickerMsg,
    instrument_id: InstrumentId,
) -> anyhow::Result<Option<FundingRateUpdate>> {
    let funding_rate = match msg.funding_rate {
        Some(rate) => rate,
        None => return Ok(None),
    };

    let (ts_event, ts_init) = parse_derivative_ticker_timestamps(msg)?;
    let rate = rust_decimal::Decimal::try_from(funding_rate)
        .with_context(|| format!("failed to convert funding rate {funding_rate} to Decimal"))?;

    Ok(Some(FundingRateUpdate::new(
        instrument_id,
        rate,
        None,
        None,
        ts_event,
        ts_init,
    )))
}

/// Parses a derivative ticker message into a mark price update.
///
/// # Errors
///
/// Returns an error if timestamp conversion fails.
pub fn parse_derivative_ticker_mark_price(
    msg: &DerivativeTickerMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
) -> anyhow::Result<Option<MarkPriceUpdate>> {
    let mark_price = match msg.mark_price {
        Some(p) => p,
        None => return Ok(None),
    };

    let (ts_event, ts_init) = parse_derivative_ticker_timestamps(msg)?;

    Ok(Some(MarkPriceUpdate::new(
        instrument_id,
        Price::new(mark_price, price_precision),
        ts_event,
        ts_init,
    )))
}

/// Parses a derivative ticker message into an index price update.
///
/// # Errors
///
/// Returns an error if timestamp conversion fails.
pub fn parse_derivative_ticker_index_price(
    msg: &DerivativeTickerMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
) -> anyhow::Result<Option<IndexPriceUpdate>> {
    let index_price = match msg.index_price {
        Some(p) => p,
        None => return Ok(None),
    };

    let (ts_event, ts_init) = parse_derivative_ticker_timestamps(msg)?;

    Ok(Some(IndexPriceUpdate::new(
        instrument_id,
        Price::new(index_price, price_precision),
        ts_event,
        ts_init,
    )))
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::AggressorSide;
    use rstest::rstest;

    use super::*;
    use crate::common::{enums::TardisExchange, testing::load_test_json};

    #[rstest]
    fn test_parse_book_change_message() {
        let json_data = load_test_json("book_change.json");
        let msg: BookChangeMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 0;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let deltas =
            parse_book_change_msg_as_deltas(&msg, price_precision, size_precision, instrument_id)
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
            parse_book_snapshot_msg_as_deltas(&msg, price_precision, size_precision, instrument_id)
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

        let depth10 = parse_book_snapshot_msg_as_depth10(
            &msg,
            price_precision,
            size_precision,
            instrument_id,
        )
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
            parse_book_snapshot_msg_as_quote(&msg, price_precision, size_precision, instrument_id)
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
        let trade = parse_trade_msg(&msg, price_precision, size_precision, instrument_id)
            .expect("Failed to parse trade message");

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("7996"));
        assert_eq!(trade.size, Quantity::from(50));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.ts_event, UnixNanos::from(1571826769669000000));
        assert_eq!(trade.ts_init, UnixNanos::from(1571826769740000000));
    }

    fn build_trade_msg_without_id() -> TradeMsg {
        let json_data = load_test_json("trade.json");
        let mut msg: TradeMsg = serde_json::from_str(&json_data).unwrap();
        msg.id = None;
        msg
    }

    #[rstest]
    fn test_parse_trade_message_derives_trade_id_when_missing() {
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

        let first = parse_trade_msg(&build_trade_msg_without_id(), 0, 0, instrument_id).unwrap();
        let second = parse_trade_msg(&build_trade_msg_without_id(), 0, 0, instrument_id).unwrap();

        assert_eq!(first.trade_id, second.trade_id, "derivation must be stable");
        assert_eq!(first.trade_id.as_str().len(), 16);

        let mut altered = build_trade_msg_without_id();
        altered.price = 7997.0;
        let altered_trade = parse_trade_msg(&altered, 0, 0, instrument_id).unwrap();
        assert_ne!(first.trade_id, altered_trade.trade_id);
    }

    #[rstest]
    fn test_parse_trade_message_derives_trade_id_when_empty() {
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");

        let mut msg = build_trade_msg_without_id();
        msg.id = Some(String::new());

        let trade = parse_trade_msg(&msg, 0, 0, instrument_id).unwrap();
        let fallback = parse_trade_msg(&build_trade_msg_without_id(), 0, 0, instrument_id).unwrap();
        assert_eq!(trade.trade_id, fallback.trade_id);
    }

    #[rstest]
    fn test_parse_bar_message() {
        let json_data = load_test_json("bar.json");
        let msg: BarMsg = serde_json::from_str(&json_data).unwrap();

        let price_precision = 1;
        let size_precision = 0;
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let bar = parse_bar_msg(&msg, price_precision, size_precision, instrument_id).unwrap();

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

        let result = parse_tardis_ws_message(ws_msg, &info, &BookSnapshotOutput::Depth10);

        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Data::Depth10(_)));
    }

    #[rstest]
    fn test_parse_tardis_ws_message_sparse_book_snapshot_routes_to_depth10() {
        let json_data = r#"{
            "type": "book_snapshot",
            "symbol": "ETC",
            "exchange": "hyperliquid",
            "name": "book_snapshot_20_10s",
            "depth": 20,
            "interval": 10000,
            "bids": [{"price": 20.002, "amount": 5.81}],
            "asks": [{"price": 20.003, "amount": 162.45}, {}],
            "timestamp": "2025-03-03T10:48:10.000Z",
            "localTimestamp": "2025-03-03T10:48:10.596818Z"
        }"#;
        let msg: BookSnapshotMsg = serde_json::from_str(json_data).unwrap();
        let ws_msg = WsMessage::BookSnapshot(msg);

        let instrument_id = InstrumentId::from("ETC.HYPERLIQUID");
        let info = Arc::new(TardisInstrumentMiniInfo::new(
            instrument_id,
            None,
            TardisExchange::Hyperliquid,
            3,
            2,
        ));

        let result = parse_tardis_ws_message(ws_msg, &info, &BookSnapshotOutput::Depth10);

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

        let result = parse_tardis_ws_message(ws_msg, &info, &BookSnapshotOutput::Deltas);

        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Data::Deltas(_)));
    }

    #[rstest]
    fn test_parse_derivative_ticker_funding_rate() {
        let json_data = load_test_json("derivative_ticker.json");
        let msg: DerivativeTickerMsg = serde_json::from_str(&json_data).unwrap();

        let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

        let result = parse_derivative_ticker_msg(&msg, instrument_id).unwrap();
        assert!(result.is_some());

        let funding = result.unwrap();
        assert_eq!(funding.instrument_id, instrument_id);
        assert_eq!(funding.rate.to_string(), "-0.00001568");
        assert!(funding.ts_event.as_u64() > 0);
        assert!(funding.ts_init.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_derivative_ticker_mark_price() {
        let json_data = load_test_json("derivative_ticker.json");
        let msg: DerivativeTickerMsg = serde_json::from_str(&json_data).unwrap();

        let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
        let price_precision = 2;

        let result =
            parse_derivative_ticker_mark_price(&msg, instrument_id, price_precision).unwrap();
        assert!(result.is_some());

        let mark = result.unwrap();
        assert_eq!(mark.instrument_id, instrument_id);
        assert_eq!(mark.value, Price::new(7987.56, price_precision));
        assert!(mark.ts_event.as_u64() > 0);
        assert!(mark.ts_init.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_derivative_ticker_index_price() {
        let json_data = load_test_json("derivative_ticker.json");
        let msg: DerivativeTickerMsg = serde_json::from_str(&json_data).unwrap();

        let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
        let price_precision = 2;

        let result =
            parse_derivative_ticker_index_price(&msg, instrument_id, price_precision).unwrap();
        assert!(result.is_some());

        let index = result.unwrap();
        assert_eq!(index.instrument_id, instrument_id);
        assert_eq!(index.value, Price::new(7989.28, price_precision));
        assert!(index.ts_event.as_u64() > 0);
        assert!(index.ts_init.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_derivative_ticker_missing_fields() {
        // Test with minimal data (only funding_rate, no mark/index)
        let json = r#"{
            "type": "derivative_ticker",
            "symbol": "BTCUSD",
            "exchange": "bitmex",
            "lastPrice": null,
            "openInterest": null,
            "fundingRate": 0.0001,
            "indexPrice": null,
            "markPrice": null,
            "timestamp": "2024-01-01T00:00:00.000Z",
            "localTimestamp": "2024-01-01T00:00:00.100Z"
        }"#;
        let msg: DerivativeTickerMsg = serde_json::from_str(json).unwrap();

        let instrument_id = InstrumentId::from("BTCUSD.BITMEX");

        let funding = parse_derivative_ticker_msg(&msg, instrument_id).unwrap();
        assert!(funding.is_some());

        let mark = parse_derivative_ticker_mark_price(&msg, instrument_id, 1).unwrap();
        assert!(mark.is_none());

        let index = parse_derivative_ticker_index_price(&msg, instrument_id, 1).unwrap();
        assert!(index.is_none());
    }
}
