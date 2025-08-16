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

use std::num::NonZero;

use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime, uuid::UUID4};
use nautilus_model::{
    data::{
        Data,
        bar::{Bar, BarSpecification, BarType},
        delta::OrderBookDelta,
        depth::{DEPTH10_LEN, OrderBookDepth10},
        funding::FundingRateUpdate,
        order::BookOrder,
        prices::{IndexPriceUpdate, MarkPriceUpdate},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{AggregationSource, BarAggregation, OrderSide, PriceType, RecordFlag},
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, TradeId, VenueOrderId},
    reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport},
    types::{
        currency::Currency,
        money::Money,
        price::Price,
        quantity::{QUANTITY_MAX, Quantity},
    },
};
use uuid::Uuid;

use super::{
    enums::{Action, WsTopic},
    messages::{
        ExecutionMsg, FundingMsg, InstrumentMsg, MarginMsg, OrderBook10Msg, OrderBookMsg, OrderMsg,
        PositionMsg, QuoteMsg, TradeBinMsg, TradeMsg, WalletMsg,
    },
};
use crate::common::parse::{
    parse_instrument_id, parse_liquidity_side, parse_optional_datetime_to_unix_nanos,
    parse_order_side, parse_order_status, parse_order_type, parse_position_side,
    parse_time_in_force,
};

/// Check if a symbol is an index symbol (starts with '.').
///
/// Index symbols in BitMEX represent indices like `.BXBT` and have different
/// behavior from regular instruments:
/// - They only have a single price value (no bid/ask spread).
/// - They don't have trades or quotes.
/// - Their price is delivered via the `lastPrice` field.
#[inline]
pub fn is_index_symbol(symbol: &str) -> bool {
    symbol.starts_with('.')
}

const BAR_SPEC_1_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_5_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(5).unwrap(),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_HOUR: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_DAY: BarSpecification = BarSpecification {
    step: NonZero::new(1).unwrap(),
    aggregation: BarAggregation::Day,
    price_type: PriceType::Last,
};

#[must_use]
pub fn parse_book_msg_vec(
    data: Vec<OrderBookMsg>,
    action: Action,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut deltas = Vec::with_capacity(data.len());
    for msg in data {
        deltas.push(Data::Delta(parse_book_msg(
            msg,
            &action,
            price_precision,
            ts_init,
        )));
    }
    deltas
}

#[must_use]
pub fn parse_book10_msg_vec(
    data: Vec<OrderBook10Msg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut depths = Vec::with_capacity(data.len());
    for msg in data {
        depths.push(Data::Depth10(Box::new(parse_book10_msg(
            msg,
            price_precision,
            ts_init,
        ))));
    }
    depths
}

#[must_use]
pub fn parse_trade_msg_vec(
    data: Vec<TradeMsg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());
    for msg in data {
        trades.push(Data::Trade(parse_trade_msg(msg, price_precision, ts_init)));
    }
    trades
}

#[must_use]
pub fn parse_trade_bin_msg_vec(
    data: Vec<TradeBinMsg>,
    topic: WsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());
    for msg in data {
        trades.push(Data::Bar(parse_trade_bin_msg(
            msg,
            &topic,
            price_precision,
            ts_init,
        )));
    }
    trades
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book_msg(
    msg: OrderBookMsg,
    action: &Action,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let flags = if action == &Action::Insert {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let instrument_id = parse_instrument_id(&msg.symbol);
    let action = action.as_book_action();
    let price = Price::new(msg.price, price_precision);
    let side = msg.side.as_order_side();
    let size = parse_quantity(msg.size.unwrap_or(0));
    let order_id = msg.id;
    let order = BookOrder::new(side, price, size, order_id);
    let sequence = 0; // Not available
    let ts_event = UnixNanos::from(msg.transact_time);

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

/// Parses an OrderBook10 message into an OrderBookDepth10 object.
///
/// # Panics
///
/// Panics if the bid or ask arrays cannot be converted to exactly 10 elements.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book10_msg(
    msg: OrderBook10Msg,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDepth10 {
    let instrument_id = parse_instrument_id(&msg.symbol);

    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);

    // Initialized with zeros
    let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
    let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];

    for (i, level) in msg.bids.iter().enumerate() {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::new(level[0], price_precision),
            Quantity::new(level[1], 0),
            0,
        );

        bids.push(bid_order);
        bid_counts[i] = 1;
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::new(level[0], price_precision),
            Quantity::new(level[1], 0),
            0,
        );

        asks.push(ask_order);
        ask_counts[i] = 1;
    }

    let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().expect("`bids` length should be 10");
    let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().expect("`asks` length should be 10");

    let ts_event = UnixNanos::from(msg.timestamp);

    OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        0, // Not applicable for BitMEX L2 books
        ts_event,
        ts_init,
    )
}

#[must_use]
pub fn parse_quote_msg(
    msg: QuoteMsg,
    last_quote: &QuoteTick,
    price_precision: u8,
    ts_init: UnixNanos,
) -> QuoteTick {
    let instrument_id = parse_instrument_id(&msg.symbol);

    let bid_price = match msg.bid_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.bid_price,
    };

    let ask_price = match msg.ask_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.ask_price,
    };

    let bid_size = match msg.bid_size {
        Some(size) => Quantity::new(std::cmp::min(QUANTITY_MAX as u64, size) as f64, 0),
        None => last_quote.bid_size,
    };

    let ask_size = match msg.ask_size {
        Some(size) => Quantity::new(std::cmp::min(QUANTITY_MAX as u64, size) as f64, 0),
        None => last_quote.ask_size,
    };

    let ts_event = UnixNanos::from(msg.timestamp);

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
pub fn parse_trade_msg(msg: TradeMsg, price_precision: u8, ts_init: UnixNanos) -> TradeTick {
    let instrument_id = parse_instrument_id(&msg.symbol);
    let price = Price::new(msg.price, price_precision);
    let size = parse_quantity(msg.size);
    let aggressor_side = msg.side.as_aggressor_side();
    let trade_id = TradeId::new(
        msg.trd_match_id
            .map(|uuid| uuid.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    );
    let ts_event = UnixNanos::from(msg.timestamp);

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
pub fn parse_trade_bin_msg(
    msg: TradeBinMsg,
    topic: &WsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Bar {
    let instrument_id = parse_instrument_id(&msg.symbol);
    let spec = bar_spec_from_topic(topic);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);
    let volume = Quantity::new(msg.volume as f64, 0);
    let ts_event = UnixNanos::from(msg.timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

#[must_use]
/// Converts a WebSocket topic to a bar specification.
///
/// # Panics
///
/// Panics if the topic is not a valid bar topic (TradeBin1m, TradeBin5m, TradeBin1h, or TradeBin1d).
pub fn bar_spec_from_topic(topic: &WsTopic) -> BarSpecification {
    match topic {
        WsTopic::TradeBin1m => BAR_SPEC_1_MINUTE,
        WsTopic::TradeBin5m => BAR_SPEC_5_MINUTE,
        WsTopic::TradeBin1h => BAR_SPEC_1_HOUR,
        WsTopic::TradeBin1d => BAR_SPEC_1_DAY,
        _ => panic!("Bar specification not supported for {topic}"),
    }
}

/// Converts a bar specification to a WebSocket topic.
///
/// # Panics
///
/// Panics if the specification is not one of the supported values (1m, 5m, 1h, or 1d).
#[must_use]
pub fn topic_from_bar_spec(spec: BarSpecification) -> WsTopic {
    match spec {
        BAR_SPEC_1_MINUTE => WsTopic::TradeBin1m,
        BAR_SPEC_5_MINUTE => WsTopic::TradeBin5m,
        BAR_SPEC_1_HOUR => WsTopic::TradeBin1h,
        BAR_SPEC_1_DAY => WsTopic::TradeBin1d,
        _ => panic!("Bar specification not supported {spec}"),
    }
}

// TODO: Use high-precision when it lands
#[must_use]
pub fn parse_quantity(value: u64) -> Quantity {
    let size_workaround = std::cmp::min(QUANTITY_MAX as u64, value);
    Quantity::new(size_workaround as f64, 0)
}

/// Parse a BitMEX WebSocket order message into a Nautilus `OrderStatusReport`.
///
/// # References
/// <https://www.bitmex.com/app/wsAPI#Order>
///
/// # Panics
///
/// Panics if required fields are missing or invalid.
pub fn parse_order_msg(msg: OrderMsg, price_precision: u8) -> OrderStatusReport {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let instrument_id = parse_instrument_id(&msg.symbol);
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let order_side = parse_order_side(&Some(crate::enums::Side::from(msg.side)));
    let order_type = parse_order_type(&msg.ord_type);
    let time_in_force = parse_time_in_force(&msg.time_in_force);
    let order_status = parse_order_status(&msg.ord_status);
    let quantity = Quantity::from(msg.order_qty);
    let filled_qty = Quantity::from(msg.cum_qty);
    let report_id = UUID4::new();
    let ts_accepted =
        parse_optional_datetime_to_unix_nanos(&Some(msg.transact_time), "transact_time");
    let ts_last = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "timestamp");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id - will be set later if present
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(report_id),
    );

    if let Some(cl_ord_id) = msg.cl_ord_id {
        report = report.with_client_order_id(ClientOrderId::new(cl_ord_id));
    }

    if let Some(cl_ord_link_id) = msg.cl_ord_link_id {
        report = report.with_order_list_id(OrderListId::new(cl_ord_link_id));
    }

    if let Some(price) = msg.price {
        report = report.with_price(Price::new(price, price_precision));
    }

    if let Some(avg_px) = msg.avg_px {
        report = report.with_avg_px(avg_px);
    }

    if let Some(trigger_price) = msg.stop_px {
        report = report.with_trigger_price(Price::new(trigger_price, price_precision));
    }

    report
}

/// Parse a BitMEX WebSocket execution message into a Nautilus `FillReport`.
///
/// # References
/// <https://www.bitmex.com/app/wsAPI#Execution>
///
/// # Panics
///
/// Panics if required fields are missing or invalid.
pub fn parse_execution_msg(msg: ExecutionMsg, price_precision: u8) -> Option<FillReport> {
    // Skip non-trade executions
    if msg.exec_type != Some(crate::enums::ExecType::Trade) {
        return None;
    }

    let account_id = AccountId::new(format!("BITMEX-{}", msg.account?));
    let instrument_id = parse_instrument_id(&msg.symbol?);
    let venue_order_id = VenueOrderId::new(msg.order_id?.to_string());
    let trade_id = TradeId::new(msg.trd_match_id?.to_string());
    let order_side = parse_order_side(&msg.side.map(crate::enums::Side::from));
    let last_qty = Quantity::from(msg.last_qty?);
    let last_px = Price::new(msg.last_px?, price_precision);
    let settlement_currency = msg.settl_currency.unwrap_or("XBT".to_string());
    let commission = Money::new(
        msg.commission.unwrap_or(0.0),
        Currency::from(settlement_currency),
    );
    let liquidity_side = parse_liquidity_side(&msg.last_liquidity_ind);
    let client_order_id = msg.cl_ord_id.map(ClientOrderId::new);
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_event = parse_optional_datetime_to_unix_nanos(&msg.transact_time, "transact_time");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    Some(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        venue_position_id,
        ts_event,
        ts_init,
        None,
    ))
}

/// Parse a BitMEX WebSocket position message into a Nautilus `PositionStatusReport`.
///
/// # References
/// <https://www.bitmex.com/app/wsAPI#Position>
pub fn parse_position_msg(msg: PositionMsg) -> PositionStatusReport {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let instrument_id = parse_instrument_id(&msg.symbol);
    let position_side = parse_position_side(msg.current_qty);
    let quantity = Quantity::from(msg.current_qty.map(|qty| qty.abs()).unwrap_or(0));
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_last = parse_optional_datetime_to_unix_nanos(&msg.timestamp, "timestamp");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        venue_position_id,
        ts_last,
        ts_init,
        None,
    )
}

/// Parse a BitMEX WebSocket wallet message.
///
/// # References
/// <https://www.bitmex.com/app/wsAPI#Wallet>
///
/// Returns the wallet data as a tuple of (account_id, currency, amount).
pub fn parse_wallet_msg(msg: WalletMsg) -> (AccountId, Currency, i64) {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let currency = Currency::from(msg.currency);
    let amount = msg.amount.unwrap_or(0);

    (account_id, currency, amount)
}

/// Parse a BitMEX WebSocket margin message.
///
/// # References
/// <https://www.bitmex.com/app/wsAPI#Margin>
///
/// Returns the margin data as a tuple of (account_id, currency, available_margin).
pub fn parse_margin_msg(msg: MarginMsg) -> (AccountId, Currency, i64) {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let currency = Currency::from(msg.currency);
    let available_margin = msg.available_margin.unwrap_or(0);

    (account_id, currency, available_margin)
}

/// Parse a BitMEX WebSocket instrument message for mark and index prices.
///
/// For index symbols (e.g., `.BXBT`):
/// - Uses the `lastPrice` field as the index price.
/// - Also emits the `markPrice` field (which equals `lastPrice` for indices).
///
/// For regular instruments:
/// - Uses the `index_price` field for index price updates.
/// - Uses the `mark_price` field for mark price updates.
///
/// Returns a Vec of Data containing mark and/or index price updates.
/// Returns an empty Vec if no relevant price is present.
pub fn parse_instrument_msg(msg: InstrumentMsg) -> Vec<Data> {
    let mut updates = Vec::new();
    let is_index = is_index_symbol(&msg.symbol);

    // For index symbols (like .BXBT), the lastPrice field contains the index price
    // For regular instruments, use the explicit index_price field if present
    let effective_index_price = if is_index {
        msg.last_price
    } else {
        msg.index_price
    };

    // Return early if no relevant prices present (mark_price or effective_index_price)
    // Note: effective_index_price uses lastPrice for index symbols, index_price for others
    // (Funding rates come through a separate Funding channel)
    if msg.mark_price.is_none() && effective_index_price.is_none() {
        return updates;
    }

    let instrument_id = InstrumentId::from(format!("{}.BITMEX", msg.symbol).as_str());
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    // Add mark price update if present
    // For index symbols, markPrice equals lastPrice and is valid to emit
    if let Some(mark_price) = msg.mark_price {
        let price = Price::from(mark_price.to_string().as_str());
        updates.push(Data::MarkPriceUpdate(MarkPriceUpdate::new(
            instrument_id,
            price,
            ts_event,
            ts_init,
        )));
    }

    // Add index price update if present
    if let Some(index_price) = effective_index_price {
        let price = Price::from(index_price.to_string().as_str());
        updates.push(Data::IndexPriceUpdate(IndexPriceUpdate::new(
            instrument_id,
            price,
            ts_event,
            ts_init,
        )));
    }

    updates
}

/// Parse a BitMEX WebSocket funding message.
///
/// Returns `Some(FundingRateUpdate)` containing funding rate information.
/// Note: This returns FundingRateUpdate directly, not wrapped in Data enum,
/// to keep it separate from the FFI layer.
pub fn parse_funding_msg(msg: FundingMsg) -> Option<FundingRateUpdate> {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    let instrument_id = InstrumentId::from(format!("{}.BITMEX", msg.symbol).as_str());
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    // Convert funding rate to Decimal
    let rate = match Decimal::from_str(&msg.funding_rate.to_string()) {
        Ok(rate) => rate,
        Err(e) => {
            tracing::error!("Failed to parse funding rate: {}", e);
            return None;
        }
    };

    Some(FundingRateUpdate::new(
        instrument_id,
        rate,
        None, // Next funding time not provided in this message
        ts_event,
        ts_init,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::quote::QuoteTick,
        enums::{AggressorSide, BookAction, LiquiditySide, PositionSide},
        identifiers::InstrumentId,
    };
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_orderbook_l2_message() {
        let json_data = load_test_json("ws_orderbook_l2.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: OrderBookMsg = serde_json::from_str(&json_data).unwrap();

        // Test Insert action
        let delta = parse_book_msg(msg.clone(), &Action::Insert, 1, UnixNanos::from(3));
        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.order.price, Price::from("98459.9"));
        assert_eq!(delta.order.size, Quantity::from(33000));
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.order.order_id, 62400580205);
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(delta.sequence, 0);
        assert_eq!(delta.ts_event, 1732436782275000000); // 2024-11-24T08:26:22.275Z in nanos
        assert_eq!(delta.ts_init, 3);

        // Test Update action (should have different flags)
        let delta = parse_book_msg(msg, &Action::Update, 1, UnixNanos::from(3));
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.action, BookAction::Update);
    }

    #[rstest]
    fn test_orderbook10_message() {
        let json_data = load_test_json("ws_orderbook_10.json");
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: OrderBook10Msg = serde_json::from_str(&json_data).unwrap();
        let depth10 = parse_book10_msg(msg, 1, UnixNanos::from(3));

        assert_eq!(depth10.instrument_id, instrument_id);

        // Check first bid level
        assert_eq!(depth10.bids[0].price, Price::from("98490.3"));
        assert_eq!(depth10.bids[0].size, Quantity::from(22400));
        assert_eq!(depth10.bids[0].side, OrderSide::Buy);

        // Check first ask level
        assert_eq!(depth10.asks[0].price, Price::from("98490.4"));
        assert_eq!(depth10.asks[0].size, Quantity::from(17600));
        assert_eq!(depth10.asks[0].side, OrderSide::Sell);

        // Check counts (should be 1 for each populated level)
        assert_eq!(depth10.bid_counts, [1; DEPTH10_LEN]);
        assert_eq!(depth10.ask_counts, [1; DEPTH10_LEN]);

        // Check flags and timestamps
        assert_eq!(depth10.sequence, 0);
        assert_eq!(depth10.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(depth10.ts_event, 1732436353513000000); // 2024-11-24T08:19:13.513Z in nanos
        assert_eq!(depth10.ts_init, 3);
    }

    #[rstest]
    fn test_quote_message() {
        let json_data = load_test_json("ws_quote.json");

        let instrument_id = InstrumentId::from("BCHUSDT.BITMEX");
        let last_quote = QuoteTick::new(
            instrument_id,
            Price::new(487.50, 2),
            Price::new(488.20, 2),
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let msg: QuoteMsg = serde_json::from_str(&json_data).unwrap();
        let quote = parse_quote_msg(msg, &last_quote, 2, UnixNanos::from(3));

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("487.55"));
        assert_eq!(quote.ask_price, Price::from("488.25"));
        assert_eq!(quote.bid_size, Quantity::from(103_000));
        assert_eq!(quote.ask_size, Quantity::from(50_000));
        assert_eq!(quote.ts_event, 1732315465085000000);
        assert_eq!(quote.ts_init, 3);
    }

    #[rstest]
    fn test_trade_message() {
        let json_data = load_test_json("ws_trade.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: TradeMsg = serde_json::from_str(&json_data).unwrap();
        let trade = parse_trade_msg(msg, 1, UnixNanos::from(3));

        assert_eq!(trade.instrument_id, instrument_id);
        assert_eq!(trade.price, Price::from("98570.9"));
        assert_eq!(trade.size, Quantity::from(100));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(
            trade.trade_id.to_string(),
            "00000000-006d-1000-0000-000e8737d536"
        );
        assert_eq!(trade.ts_event, 1732436138704000000); // 2024-11-24T08:15:38.704Z in nanos
        assert_eq!(trade.ts_init, 3);
    }

    #[rstest]
    fn test_trade_bin_message() {
        let json_data = load_test_json("ws_trade_bin_1m.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let topic = WsTopic::TradeBin1m;

        let msg: TradeBinMsg = serde_json::from_str(&json_data).unwrap();
        let bar = parse_trade_bin_msg(msg, &topic, 1, UnixNanos::from(3));

        assert_eq!(bar.instrument_id(), instrument_id);
        assert_eq!(
            bar.bar_type.spec(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
        );
        assert_eq!(bar.open, Price::from("97550.0"));
        assert_eq!(bar.high, Price::from("97584.4"));
        assert_eq!(bar.low, Price::from("97550.0"));
        assert_eq!(bar.close, Price::from("97570.1"));
        assert_eq!(bar.volume, Quantity::from(84_000));
        assert_eq!(bar.ts_event, 1732392420000000000); // 2024-11-23T20:07:00.000Z in nanos
        assert_eq!(bar.ts_init, 3);
    }

    #[rstest]
    fn test_parse_order_msg() {
        let json_data = load_test_json("ws_order.json");
        let msg: OrderMsg = serde_json::from_str(&json_data).unwrap();
        let report = parse_order_msg(msg, 1);

        assert_eq!(report.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(report.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(
            report.venue_order_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440001"
        );
        assert_eq!(
            report.client_order_id.unwrap().to_string(),
            "mm_bitmex_1a/oemUeQ4CAJZgP3fjHsA"
        );
        assert_eq!(report.order_side, nautilus_model::enums::OrderSide::Buy);
        assert_eq!(report.order_type, nautilus_model::enums::OrderType::Limit);
        assert_eq!(
            report.time_in_force,
            nautilus_model::enums::TimeInForce::Gtc
        );
        assert_eq!(
            report.order_status,
            nautilus_model::enums::OrderStatus::Accepted
        );
        assert_eq!(report.quantity, Quantity::from(100));
        assert_eq!(report.filled_qty, Quantity::from(0));
        assert_eq!(report.price.unwrap(), Price::from("98000.0"));
        assert_eq!(report.ts_accepted, 1732530600000000000); // 2024-11-25T10:30:00.000Z
    }

    #[rstest]
    fn test_parse_execution_msg() {
        let json_data = load_test_json("ws_execution.json");
        let msg: ExecutionMsg = serde_json::from_str(&json_data).unwrap();
        let fill = parse_execution_msg(msg, 1).unwrap();

        assert_eq!(fill.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(fill.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(
            fill.venue_order_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440002"
        );
        assert_eq!(
            fill.trade_id.to_string(),
            "00000000-006d-1000-0000-000e8737d540"
        );
        assert_eq!(
            fill.client_order_id.unwrap().to_string(),
            "mm_bitmex_2b/oemUeQ4CAJZgP3fjHsB"
        );
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::from(100));
        assert_eq!(fill.last_px, Price::from("98950.0"));
        assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
        assert_eq!(fill.commission, Money::new(0.00075, Currency::from("XBT")));
        assert_eq!(fill.commission.currency.code.to_string(), "XBT");
        assert_eq!(fill.ts_event, 1732530900789000000); // 2024-11-25T10:35:00.789Z
    }

    #[rstest]
    fn test_parse_execution_msg_non_trade() {
        // Test that non-trade executions return None
        let mut msg: ExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(crate::enums::ExecType::Settlement);

        let result = parse_execution_msg(msg, 1);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_position_msg() {
        let json_data = load_test_json("ws_position.json");
        let msg: PositionMsg = serde_json::from_str(&json_data).unwrap();
        let report = parse_position_msg(msg);

        assert_eq!(report.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(report.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(report.position_side, PositionSide::Long);
        assert_eq!(report.quantity, Quantity::from(1000));
        assert!(report.venue_position_id.is_none());
        assert_eq!(report.ts_last, 1732530900789000000); // 2024-11-25T10:35:00.789Z
    }

    #[rstest]
    fn test_parse_position_msg_short() {
        let mut msg: PositionMsg =
            serde_json::from_str(&load_test_json("ws_position.json")).unwrap();
        msg.current_qty = Some(-500);

        let report = parse_position_msg(msg);
        assert_eq!(report.position_side, PositionSide::Short);
        assert_eq!(report.quantity, Quantity::from(500));
    }

    #[rstest]
    fn test_parse_position_msg_flat() {
        let mut msg: PositionMsg =
            serde_json::from_str(&load_test_json("ws_position.json")).unwrap();
        msg.current_qty = Some(0);

        let report = parse_position_msg(msg);
        assert_eq!(report.position_side, PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::from(0));
    }

    #[rstest]
    fn test_parse_wallet_msg() {
        let json_data = load_test_json("ws_wallet.json");
        let msg: WalletMsg = serde_json::from_str(&json_data).unwrap();
        let (account_id, currency, amount) = parse_wallet_msg(msg);

        assert_eq!(account_id.to_string(), "BITMEX-1234567");
        assert_eq!(currency.code.to_string(), "XBT");
        assert_eq!(amount, 100005180);
    }

    #[rstest]
    fn test_parse_wallet_msg_no_amount() {
        let mut msg: WalletMsg = serde_json::from_str(&load_test_json("ws_wallet.json")).unwrap();
        msg.amount = None;

        let (_, _, amount) = parse_wallet_msg(msg);
        assert_eq!(amount, 0);
    }

    #[rstest]
    fn test_parse_margin_msg() {
        let json_data = load_test_json("ws_margin.json");
        let msg: MarginMsg = serde_json::from_str(&json_data).unwrap();
        let (account_id, currency, available_margin) = parse_margin_msg(msg);

        assert_eq!(account_id.to_string(), "BITMEX-1234567");
        assert_eq!(currency.code.to_string(), "XBT");
        assert_eq!(available_margin, 99994411);
    }

    #[rstest]
    fn test_parse_margin_msg_no_available() {
        let mut msg: MarginMsg = serde_json::from_str(&load_test_json("ws_margin.json")).unwrap();
        msg.available_margin = None;

        let (_, _, available_margin) = parse_margin_msg(msg);
        assert_eq!(available_margin, 0);
    }

    #[rstest]
    fn test_parse_instrument_msg_both_prices() {
        let json_data = load_test_json("ws_instrument.json");
        let msg: InstrumentMsg = serde_json::from_str(&json_data).unwrap();
        let updates = parse_instrument_msg(msg);

        // XBTUSD is not an index symbol, so it should have both mark and index prices
        assert_eq!(updates.len(), 2);

        // Check mark price update
        match &updates[0] {
            Data::MarkPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
                assert_eq!(update.value.as_f64(), 95125.7);
            }
            _ => panic!("Expected MarkPriceUpdate at index 0"),
        }

        // Check index price update
        match &updates[1] {
            Data::IndexPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
                assert_eq!(update.value.as_f64(), 95124.3);
            }
            _ => panic!("Expected IndexPriceUpdate at index 1"),
        }
    }

    #[rstest]
    fn test_parse_instrument_msg_mark_price_only() {
        let mut msg: InstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.index_price = None;

        let updates = parse_instrument_msg(msg);

        assert_eq!(updates.len(), 1);
        match &updates[0] {
            Data::MarkPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
                assert_eq!(update.value.as_f64(), 95125.7);
            }
            _ => panic!("Expected MarkPriceUpdate"),
        }
    }

    #[rstest]
    fn test_parse_instrument_msg_index_price_only() {
        let mut msg: InstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.mark_price = None;

        let updates = parse_instrument_msg(msg);

        assert_eq!(updates.len(), 1);
        match &updates[0] {
            Data::IndexPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
                assert_eq!(update.value.as_f64(), 95124.3);
            }
            _ => panic!("Expected IndexPriceUpdate"),
        }
    }

    #[rstest]
    fn test_parse_instrument_msg_no_prices() {
        let mut msg: InstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.mark_price = None;
        msg.index_price = None;
        msg.last_price = None;

        let updates = parse_instrument_msg(msg);
        assert_eq!(updates.len(), 0);
    }

    #[rstest]
    fn test_parse_instrument_msg_index_symbol() {
        // Test for index symbols like .BXBT where lastPrice is the index price
        // and markPrice equals lastPrice
        let mut msg: InstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.symbol = ".BXBT".to_string();
        msg.last_price = Some(119163.05);
        msg.mark_price = Some(119163.05); // Index symbols have mark price equal to last price
        msg.index_price = None;

        let updates = parse_instrument_msg(msg);

        assert_eq!(updates.len(), 2);

        // Check mark price update
        match &updates[0] {
            Data::MarkPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), ".BXBT.BITMEX");
                assert_eq!(update.value, Price::from("119163.05"));
            }
            _ => panic!("Expected MarkPriceUpdate for index symbol"),
        }

        // Check index price update
        match &updates[1] {
            Data::IndexPriceUpdate(update) => {
                assert_eq!(update.instrument_id.to_string(), ".BXBT.BITMEX");
                assert_eq!(update.value, Price::from("119163.05"));
            }
            _ => panic!("Expected IndexPriceUpdate for index symbol"),
        }
    }

    #[rstest]
    fn test_parse_funding_msg() {
        let json_data = load_test_json("ws_funding_rate.json");
        let msg: FundingMsg = serde_json::from_str(&json_data).unwrap();
        let update = parse_funding_msg(msg);

        assert!(update.is_some());
        let update = update.unwrap();

        assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(update.rate.to_string(), "0.0001");
        assert!(update.next_funding_ns.is_none());
    }
}
