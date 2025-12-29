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

//! Parsing functions for converting Deribit WebSocket messages to Nautilus domain types.

use ahash::AHashMap;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    data::{BookOrder, Data, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::TradeId,
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use ustr::Ustr;

use super::{
    enums::{DeribitBookAction, DeribitBookMsgType},
    messages::{DeribitBookMsg, DeribitQuoteMsg, DeribitTickerMsg, DeribitTradeMsg},
};

/// Parses a Deribit trade message into a Nautilus `TradeTick`.
///
/// # Errors
///
/// Returns an error if the trade cannot be parsed.
pub fn parse_trade_msg(
    msg: &DeribitTradeMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = Price::new(msg.price, price_precision);
    let size = Quantity::new(msg.amount.abs(), size_precision);

    let aggressor_side = match msg.direction.as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new(&msg.trade_id);
    let ts_event = UnixNanos::new(msg.timestamp * NANOSECONDS_IN_MILLISECOND);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a vector of Deribit trade messages into Nautilus `Data` items.
pub fn parse_trades_data(
    trades: Vec<DeribitTradeMsg>,
    instruments_cache: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
    trades
        .iter()
        .filter_map(|msg| {
            instruments_cache
                .get(&msg.instrument_name)
                .and_then(|inst| parse_trade_msg(msg, inst, ts_init).ok())
                .map(Data::Trade)
        })
        .collect()
}

/// Converts a Deribit book action to Nautilus `BookAction`.
#[allow(dead_code)] // Reserved for future structured book parsing
fn convert_book_action(action: &DeribitBookAction) -> BookAction {
    match action {
        DeribitBookAction::New => BookAction::Add,
        DeribitBookAction::Change => BookAction::Update,
        DeribitBookAction::Delete => BookAction::Delete,
    }
}

/// Parses a Deribit order book snapshot into Nautilus `OrderBookDeltas`.
///
/// # Errors
///
/// Returns an error if the book data cannot be parsed.
pub fn parse_book_snapshot(
    msg: &DeribitBookMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = UnixNanos::new(msg.timestamp * NANOSECONDS_IN_MILLISECOND);

    let mut deltas = Vec::new();

    // Add CLEAR action first for snapshot
    deltas.push(OrderBookDelta::clear(
        instrument_id,
        msg.change_id,
        ts_event,
        ts_init,
    ));

    // Parse bids: ["new", price, amount] for snapshot (3-element format)
    for (i, bid) in msg.bids.iter().enumerate() {
        if bid.len() >= 3 {
            // Skip action field (bid[0]), use bid[1] for price and bid[2] for amount
            let price_val = bid[1].as_f64().unwrap_or(0.0);
            let amount_val = bid[2].as_f64().unwrap_or(0.0);

            if amount_val > 0.0 {
                let price = Price::new(price_val, price_precision);
                let size = Quantity::new(amount_val, size_precision);

                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    BookOrder::new(OrderSide::Buy, price, size, i as u64),
                    0, // No flags for regular deltas
                    msg.change_id,
                    ts_event,
                    ts_init,
                ));
            }
        }
    }

    // Parse asks: ["new", price, amount] for snapshot (3-element format)
    let num_bids = msg.bids.len();
    for (i, ask) in msg.asks.iter().enumerate() {
        if ask.len() >= 3 {
            // Skip action field (ask[0]), use ask[1] for price and ask[2] for amount
            let price_val = ask[1].as_f64().unwrap_or(0.0);
            let amount_val = ask[2].as_f64().unwrap_or(0.0);

            if amount_val > 0.0 {
                let price = Price::new(price_val, price_precision);
                let size = Quantity::new(amount_val, size_precision);

                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    BookOrder::new(OrderSide::Sell, price, size, (num_bids + i) as u64),
                    0, // No flags for regular deltas
                    msg.change_id,
                    ts_event,
                    ts_init,
                ));
            }
        }
    }

    // Set F_LAST flag on the last delta
    if let Some(last) = deltas.last_mut() {
        *last = OrderBookDelta::new(
            last.instrument_id,
            last.action,
            last.order,
            RecordFlag::F_LAST as u8,
            last.sequence,
            last.ts_event,
            last.ts_init,
        );
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a Deribit order book change (delta) into Nautilus `OrderBookDeltas`.
///
/// # Errors
///
/// Returns an error if the book data cannot be parsed.
pub fn parse_book_delta(
    msg: &DeribitBookMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let ts_event = UnixNanos::new(msg.timestamp * NANOSECONDS_IN_MILLISECOND);

    let mut deltas = Vec::new();

    // Parse bids: [action, price, amount] for delta
    for (i, bid) in msg.bids.iter().enumerate() {
        if bid.len() >= 3 {
            let action_str = bid[0].as_str().unwrap_or("new");
            let price_val = bid[1].as_f64().unwrap_or(0.0);
            let amount_val = bid[2].as_f64().unwrap_or(0.0);

            let action = match action_str {
                "new" => BookAction::Add,
                "change" => BookAction::Update,
                "delete" => BookAction::Delete,
                _ => continue,
            };

            let price = Price::new(price_val, price_precision);
            let size = Quantity::new(amount_val.abs(), size_precision);

            deltas.push(OrderBookDelta::new(
                instrument_id,
                action,
                BookOrder::new(OrderSide::Buy, price, size, i as u64),
                0, // No flags for regular deltas
                msg.change_id,
                ts_event,
                ts_init,
            ));
        }
    }

    // Parse asks: [action, price, amount] for delta
    let num_bids = msg.bids.len();
    for (i, ask) in msg.asks.iter().enumerate() {
        if ask.len() >= 3 {
            let action_str = ask[0].as_str().unwrap_or("new");
            let price_val = ask[1].as_f64().unwrap_or(0.0);
            let amount_val = ask[2].as_f64().unwrap_or(0.0);

            let action = match action_str {
                "new" => BookAction::Add,
                "change" => BookAction::Update,
                "delete" => BookAction::Delete,
                _ => continue,
            };

            let price = Price::new(price_val, price_precision);
            let size = Quantity::new(amount_val.abs(), size_precision);

            deltas.push(OrderBookDelta::new(
                instrument_id,
                action,
                BookOrder::new(OrderSide::Sell, price, size, (num_bids + i) as u64),
                0, // No flags for regular deltas
                msg.change_id,
                ts_event,
                ts_init,
            ));
        }
    }

    // Set F_LAST flag on the last delta
    if let Some(last) = deltas.last_mut() {
        *last = OrderBookDelta::new(
            last.instrument_id,
            last.action,
            last.order,
            RecordFlag::F_LAST as u8,
            last.sequence,
            last.ts_event,
            last.ts_init,
        );
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

/// Parses a Deribit order book message (snapshot or delta) into Nautilus `OrderBookDeltas`.
///
/// # Errors
///
/// Returns an error if the book data cannot be parsed.
pub fn parse_book_msg(
    msg: &DeribitBookMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    match msg.msg_type {
        DeribitBookMsgType::Snapshot => parse_book_snapshot(msg, instrument, ts_init),
        DeribitBookMsgType::Change => parse_book_delta(msg, instrument, ts_init),
    }
}

/// Parses a Deribit ticker message into a Nautilus `QuoteTick`.
///
/// # Errors
///
/// Returns an error if the quote cannot be parsed.
pub fn parse_ticker_to_quote(
    msg: &DeribitTickerMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = Price::new(msg.best_bid_price.unwrap_or(0.0), price_precision);
    let ask_price = Price::new(msg.best_ask_price.unwrap_or(0.0), price_precision);
    let bid_size = Quantity::new(msg.best_bid_amount.unwrap_or(0.0), size_precision);
    let ask_size = Quantity::new(msg.best_ask_amount.unwrap_or(0.0), size_precision);
    let ts_event = UnixNanos::new(msg.timestamp * NANOSECONDS_IN_MILLISECOND);

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

/// Parses a Deribit quote message into a Nautilus `QuoteTick`.
///
/// # Errors
///
/// Returns an error if the quote cannot be parsed.
pub fn parse_quote_msg(
    msg: &DeribitQuoteMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let bid_price = Price::new(msg.best_bid_price, price_precision);
    let ask_price = Price::new(msg.best_ask_price, price_precision);
    let bid_size = Quantity::new(msg.best_bid_amount, size_precision);
    let ask_size = Quantity::new(msg.best_ask_amount, size_precision);
    let ts_event = UnixNanos::new(msg.timestamp * NANOSECONDS_IN_MILLISECOND);

    QuoteTick::new_checked(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{parse::parse_deribit_instrument_any, testing::load_test_json},
        http::models::{DeribitInstrument, DeribitJsonRpcResponse},
    };

    /// Helper function to create a test instrument (BTC-PERPETUAL).
    fn test_perpetual_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments.json");
        let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
            serde_json::from_str(&json).unwrap();
        let instrument = &response.result.unwrap()[0];
        parse_deribit_instrument_any(instrument, UnixNanos::default(), UnixNanos::default())
            .unwrap()
            .unwrap()
    }

    #[rstest]
    fn test_parse_trade_msg_sell() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_trades.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let trades: Vec<DeribitTradeMsg> =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();
        let msg = &trades[0];

        let tick = parse_trade_msg(msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(92294.5));
        assert_eq!(tick.size, instrument.make_qty(10.0, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.trade_id.to_string(), "403691824");
        assert_eq!(tick.ts_event, UnixNanos::new(1_765_531_356_452_000_000));
    }

    #[rstest]
    fn test_parse_trade_msg_buy() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_trades.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let trades: Vec<DeribitTradeMsg> =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();
        let msg = &trades[1];

        let tick = parse_trade_msg(msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(92288.5));
        assert_eq!(tick.size, instrument.make_qty(750.0, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(tick.trade_id.to_string(), "403691825");
    }

    #[rstest]
    fn test_parse_book_snapshot() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_book_snapshot.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitBookMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        let deltas = parse_book_snapshot(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        // Should have CLEAR + 5 bids + 5 asks = 11 deltas
        assert_eq!(deltas.deltas.len(), 11);

        // First delta should be CLEAR
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Check first bid
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.action, BookAction::Add);
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.order.price, instrument.make_price(42500.0));
        assert_eq!(first_bid.order.size, instrument.make_qty(1000.0, None));

        // Check first ask
        let first_ask = &deltas.deltas[6];
        assert_eq!(first_ask.action, BookAction::Add);
        assert_eq!(first_ask.order.side, OrderSide::Sell);
        assert_eq!(first_ask.order.price, instrument.make_price(42501.0));
        assert_eq!(first_ask.order.size, instrument.make_qty(800.0, None));

        // Check F_LAST flag on last delta
        let last = deltas.deltas.last().unwrap();
        assert_eq!(
            last.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_book_delta() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_book_delta.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitBookMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        let deltas = parse_book_delta(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        // Should have 2 bid deltas + 2 ask deltas = 4 deltas
        assert_eq!(deltas.deltas.len(), 4);

        // Check first bid - "change" action
        let bid_change = &deltas.deltas[0];
        assert_eq!(bid_change.action, BookAction::Update);
        assert_eq!(bid_change.order.side, OrderSide::Buy);
        assert_eq!(bid_change.order.price, instrument.make_price(42500.0));
        assert_eq!(bid_change.order.size, instrument.make_qty(950.0, None));

        // Check second bid - "new" action
        let bid_new = &deltas.deltas[1];
        assert_eq!(bid_new.action, BookAction::Add);
        assert_eq!(bid_new.order.side, OrderSide::Buy);
        assert_eq!(bid_new.order.price, instrument.make_price(42498.5));
        assert_eq!(bid_new.order.size, instrument.make_qty(300.0, None));

        // Check first ask - "delete" action
        let ask_delete = &deltas.deltas[2];
        assert_eq!(ask_delete.action, BookAction::Delete);
        assert_eq!(ask_delete.order.side, OrderSide::Sell);
        assert_eq!(ask_delete.order.price, instrument.make_price(42501.0));
        assert_eq!(ask_delete.order.size, instrument.make_qty(0.0, None));

        // Check second ask - "change" action
        let ask_change = &deltas.deltas[3];
        assert_eq!(ask_change.action, BookAction::Update);
        assert_eq!(ask_change.order.side, OrderSide::Sell);
        assert_eq!(ask_change.order.price, instrument.make_price(42501.5));
        assert_eq!(ask_change.order.size, instrument.make_qty(700.0, None));

        // Check F_LAST flag on last delta
        let last = deltas.deltas.last().unwrap();
        assert_eq!(
            last.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_ticker_to_quote() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_ticker.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitTickerMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        // Verify the message was deserialized correctly
        assert_eq!(msg.instrument_name.as_str(), "BTC-PERPETUAL");
        assert_eq!(msg.timestamp, 1_765_541_474_086);
        assert_eq!(msg.best_bid_price, Some(92283.5));
        assert_eq!(msg.best_ask_price, Some(92284.0));
        assert_eq!(msg.best_bid_amount, Some(117660.0));
        assert_eq!(msg.best_ask_amount, Some(186520.0));
        assert_eq!(msg.mark_price, 92281.78);
        assert_eq!(msg.index_price, 92263.55);
        assert_eq!(msg.open_interest, 1132329370.0);

        let quote = parse_ticker_to_quote(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(92283.5));
        assert_eq!(quote.ask_price, instrument.make_price(92284.0));
        assert_eq!(quote.bid_size, instrument.make_qty(117660.0, None));
        assert_eq!(quote.ask_size, instrument.make_qty(186520.0, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_765_541_474_086_000_000));
    }

    #[rstest]
    fn test_parse_quote_msg() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_quote.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitQuoteMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        // Verify the message was deserialized correctly
        assert_eq!(msg.instrument_name.as_str(), "BTC-PERPETUAL");
        assert_eq!(msg.timestamp, 1_765_541_767_174);
        assert_eq!(msg.best_bid_price, 92288.0);
        assert_eq!(msg.best_ask_price, 92288.5);
        assert_eq!(msg.best_bid_amount, 133440.0);
        assert_eq!(msg.best_ask_amount, 99470.0);

        let quote = parse_quote_msg(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(92288.0));
        assert_eq!(quote.ask_price, instrument.make_price(92288.5));
        assert_eq!(quote.bid_size, instrument.make_qty(133440.0, None));
        assert_eq!(quote.ask_size, instrument.make_qty(99470.0, None));
        assert_eq!(quote.ts_event, UnixNanos::new(1_765_541_767_174_000_000));
    }

    #[rstest]
    fn test_parse_book_msg_snapshot() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_book_snapshot.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitBookMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        // Validate raw message format - snapshots use 3-element arrays: ["new", price, amount]
        assert_eq!(
            msg.bids[0].len(),
            3,
            "Snapshot bids should have 3 elements: [action, price, amount]"
        );
        assert_eq!(
            msg.bids[0][0].as_str(),
            Some("new"),
            "First element should be 'new' action for snapshot"
        );
        assert_eq!(
            msg.asks[0].len(),
            3,
            "Snapshot asks should have 3 elements: [action, price, amount]"
        );
        assert_eq!(
            msg.asks[0][0].as_str(),
            Some("new"),
            "First element should be 'new' action for snapshot"
        );

        let deltas = parse_book_msg(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        // Should have CLEAR + 5 bids + 5 asks = 11 deltas
        assert_eq!(deltas.deltas.len(), 11);

        // First delta should be CLEAR
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Verify first bid was parsed correctly from ["new", 42500.0, 1000.0]
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.action, BookAction::Add);
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.order.price, instrument.make_price(42500.0));
        assert_eq!(first_bid.order.size, instrument.make_qty(1000.0, None));

        // Verify first ask was parsed correctly from ["new", 42501.0, 800.0]
        let first_ask = &deltas.deltas[6];
        assert_eq!(first_ask.action, BookAction::Add);
        assert_eq!(first_ask.order.side, OrderSide::Sell);
        assert_eq!(first_ask.order.price, instrument.make_price(42501.0));
        assert_eq!(first_ask.order.size, instrument.make_qty(800.0, None));
    }

    #[rstest]
    fn test_parse_book_msg_delta() {
        let instrument = test_perpetual_instrument();
        let json = load_test_json("ws_book_delta.json");
        let response: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg: DeribitBookMsg =
            serde_json::from_value(response["params"]["data"].clone()).unwrap();

        // Validate raw message format - deltas use 3-element arrays: [action, price, amount]
        assert_eq!(
            msg.bids[0].len(),
            3,
            "Delta bids should have 3 elements: [action, price, amount]"
        );
        assert_eq!(
            msg.bids[0][0].as_str(),
            Some("change"),
            "First bid should be 'change' action"
        );
        assert_eq!(
            msg.bids[1][0].as_str(),
            Some("new"),
            "Second bid should be 'new' action"
        );
        assert_eq!(
            msg.asks[0].len(),
            3,
            "Delta asks should have 3 elements: [action, price, amount]"
        );
        assert_eq!(
            msg.asks[0][0].as_str(),
            Some("delete"),
            "First ask should be 'delete' action"
        );

        let deltas = parse_book_msg(&msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        // Should have 2 bid deltas + 2 ask deltas = 4 deltas
        assert_eq!(deltas.deltas.len(), 4);

        // Delta should not have CLEAR action
        assert_ne!(deltas.deltas[0].action, BookAction::Clear);

        // Verify first bid "change" action was parsed correctly from ["change", 42500.0, 950.0]
        let bid_change = &deltas.deltas[0];
        assert_eq!(bid_change.action, BookAction::Update);
        assert_eq!(bid_change.order.side, OrderSide::Buy);
        assert_eq!(bid_change.order.price, instrument.make_price(42500.0));
        assert_eq!(bid_change.order.size, instrument.make_qty(950.0, None));

        // Verify second bid "new" action was parsed correctly from ["new", 42498.5, 300.0]
        let bid_new = &deltas.deltas[1];
        assert_eq!(bid_new.action, BookAction::Add);
        assert_eq!(bid_new.order.side, OrderSide::Buy);
        assert_eq!(bid_new.order.price, instrument.make_price(42498.5));
        assert_eq!(bid_new.order.size, instrument.make_qty(300.0, None));

        // Verify first ask "delete" action was parsed correctly from ["delete", 42501.0, 0.0]
        let ask_delete = &deltas.deltas[2];
        assert_eq!(ask_delete.action, BookAction::Delete);
        assert_eq!(ask_delete.order.side, OrderSide::Sell);
        assert_eq!(ask_delete.order.price, instrument.make_price(42501.0));

        // Verify second ask "change" action was parsed correctly from ["change", 42501.5, 700.0]
        let ask_change = &deltas.deltas[3];
        assert_eq!(ask_change.action, BookAction::Update);
        assert_eq!(ask_change.order.side, OrderSide::Sell);
        assert_eq!(ask_change.order.price, instrument.make_price(42501.5));
        assert_eq!(ask_change.order.size, instrument.make_qty(700.0, None));
    }
}
