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

use chrono::{DateTime, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API,
        QuoteTick, TradeTick,
    },
    enums::{AggregationSource, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use uuid::Uuid;

use super::{
    message::{BarMsg, BookChangeMsg, BookLevel, BookSnapshotMsg, TradeMsg, WsMessage},
    types::InstrumentMiniInfo,
};
use crate::parse::{normalize_amount, parse_aggressor_side, parse_bar_spec, parse_book_action};

#[must_use]
pub fn parse_tardis_ws_message(msg: WsMessage, info: Arc<InstrumentMiniInfo>) -> Option<Data> {
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
            Some(Data::Deltas(parse_book_change_msg_as_deltas(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            )))
        }
        WsMessage::BookSnapshot(msg) => match msg.bids.len() {
            1 => Some(Data::Quote(parse_book_snapshot_msg_as_quote(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ))),
            _ => Some(Data::Deltas(parse_book_snapshot_msg_as_deltas(
                msg,
                info.price_precision,
                info.size_precision,
                info.instrument_id,
            ))),
        },
        WsMessage::Trade(msg) => Some(Data::Trade(parse_trade_msg(
            msg,
            info.price_precision,
            info.size_precision,
            info.instrument_id,
        ))),
        WsMessage::TradeBar(msg) => Some(Data::Bar(parse_bar_msg(
            msg,
            info.price_precision,
            info.size_precision,
            info.instrument_id,
        ))),
        WsMessage::DerivativeTicker(_) => None,
        WsMessage::Disconnect(_) => None,
    }
}

#[must_use]
pub fn parse_book_change_msg_as_deltas(
    msg: BookChangeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> OrderBookDeltas_API {
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

#[must_use]
pub fn parse_book_snapshot_msg_as_deltas(
    msg: BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> OrderBookDeltas_API {
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

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book_msg_as_deltas(
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    is_snapshot: bool,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
    timestamp: DateTime<Utc>,
    local_timestamp: DateTime<Utc>,
) -> OrderBookDeltas_API {
    let ts_event = UnixNanos::from(timestamp.timestamp_nanos_opt().unwrap() as u64);
    let ts_init = UnixNanos::from(local_timestamp.timestamp_nanos_opt().unwrap() as u64);

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
    OrderBookDeltas_API::new(OrderBookDeltas::new(instrument_id, deltas))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
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

#[must_use]
pub fn parse_book_snapshot_msg_as_quote(
    msg: BookSnapshotMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> QuoteTick {
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    let best_bid = &msg.bids[0];
    let bid_price = Price::new(best_bid.price, price_precision);
    let bid_size = Quantity::new(best_bid.amount, size_precision);

    let best_ask = &msg.asks[0];
    let ask_price = Price::new(best_ask.price, price_precision);
    let ask_size = Quantity::new(best_ask.amount, size_precision);

    QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[must_use]
pub fn parse_trade_msg(
    msg: TradeMsg,
    price_precision: u8,
    size_precision: u8,
    instrument_id: InstrumentId,
) -> TradeTick {
    let price = Price::new(msg.price, price_precision);
    let size = Quantity::new(msg.amount, size_precision);
    let aggressor_side = parse_aggressor_side(&msg.side);
    let trade_id = TradeId::new(msg.id.unwrap_or_else(|| Uuid::new_v4().to_string()));
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
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
    let volume = Quantity::new(msg.volume, size_precision);
    let ts_event = UnixNanos::from(msg.timestamp);
    let ts_init = UnixNanos::from(msg.local_timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::enums::{AggressorSide, BookAction};
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
            parse_book_change_msg_as_deltas(msg, price_precision, size_precision, instrument_id);

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
            parse_book_snapshot_msg_as_deltas(msg, price_precision, size_precision, instrument_id);
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
            parse_book_snapshot_msg_as_quote(msg, price_precision, size_precision, instrument_id);

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
        let trade = parse_trade_msg(msg, price_precision, size_precision, instrument_id);

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
