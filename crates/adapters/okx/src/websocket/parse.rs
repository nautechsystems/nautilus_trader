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

//! Functions translating raw OKX WebSocket frames into Nautilus data types.

use ahash::AHashMap;
use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10,
        QuoteTick, TradeTick, depth::DEPTH10_LEN,
    },
    enums::{
        AggregationSource, AggressorSide, BookAction, LiquiditySide, OrderSide, OrderStatus,
        OrderType, RecordFlag, TimeInForce, TriggerType,
    },
    identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    enums::OKXWsChannel,
    messages::{
        OKXAlgoOrderMsg, OKXBookMsg, OKXCandleMsg, OKXIndexPriceMsg, OKXMarkPriceMsg, OKXOrderMsg,
        OKXTickerMsg, OKXTradeMsg, OrderBookEntry,
    },
};
use crate::{
    common::{
        consts::{OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE},
        enums::{
            OKXBookAction, OKXCandleConfirm, OKXInstrumentType, OKXOrderCategory, OKXOrderStatus,
            OKXOrderType, OKXSide, OKXTargetCurrency, OKXTriggerType,
        },
        models::OKXInstrument,
        parse::{
            okx_channel_to_bar_spec, parse_client_order_id, parse_fee, parse_fee_currency,
            parse_funding_rate_msg, parse_instrument_any, parse_message_vec,
            parse_millisecond_timestamp, parse_price, parse_quantity,
        },
    },
    websocket::messages::{ExecutionReport, NautilusWsMessage, OKXFundingRateMsg},
};

/// Checks if a price string indicates market execution.
///
/// OKX uses special sentinel values for market price:
/// - "" (empty string)
/// - "0"
/// - "-1" (market price)
/// - "-2" (market price with protection)
fn is_market_price(px: &str) -> bool {
    px.is_empty() || px == "0" || px == "-1" || px == "-2"
}

/// Extracts fee rates from a cached instrument.
///
/// Returns a tuple of (margin_init, margin_maint, maker_fee, taker_fee).
/// All values are None if the instrument type doesn't support fees.
fn extract_fees_from_cached_instrument(
    instrument: &InstrumentAny,
) -> (
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
) {
    match instrument {
        InstrumentAny::CurrencyPair(pair) => (
            Some(pair.margin_init),
            Some(pair.margin_maint),
            Some(pair.maker_fee),
            Some(pair.taker_fee),
        ),
        InstrumentAny::CryptoPerpetual(perp) => (
            Some(perp.margin_init),
            Some(perp.margin_maint),
            Some(perp.maker_fee),
            Some(perp.taker_fee),
        ),
        InstrumentAny::CryptoFuture(future) => (
            Some(future.margin_init),
            Some(future.margin_maint),
            Some(future.maker_fee),
            Some(future.taker_fee),
        ),
        InstrumentAny::CryptoOption(option) => (
            Some(option.margin_init),
            Some(option.margin_maint),
            Some(option.maker_fee),
            Some(option.taker_fee),
        ),
        _ => (None, None, None, None),
    }
}

/// Parses vector of OKX book messages into Nautilus order book deltas.
///
/// # Errors
///
/// Returns an error if any underlying book message cannot be parsed.
pub fn parse_book_msg_vec(
    data: Vec<OKXBookMsg>,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    action: OKXBookAction,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let mut deltas = Vec::with_capacity(data.len());

    for msg in data {
        let deltas_api = OrderBookDeltas_API::new(parse_book_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            &action,
            ts_init,
        )?);
        deltas.push(Data::Deltas(deltas_api));
    }

    Ok(deltas)
}

/// Parses vector of OKX ticker messages into Nautilus quote ticks.
///
/// # Errors
///
/// Returns an error if any ticker message fails to parse.
pub fn parse_ticker_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    parse_message_vec(
        data,
        |msg| {
            parse_ticker_msg(
                msg,
                *instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )
        },
        Data::Quote,
    )
}

/// Parses vector of OKX book messages into Nautilus quote ticks.
///
/// # Errors
///
/// Returns an error if any quote message fails to parse.
pub fn parse_quote_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    parse_message_vec(
        data,
        |msg| {
            parse_quote_msg(
                msg,
                *instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )
        },
        Data::Quote,
    )
}

/// Parses vector of OKX trade messages into Nautilus trade ticks.
///
/// # Errors
///
/// Returns an error if any trade message fails to parse.
pub fn parse_trade_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    parse_message_vec(
        data,
        |msg| {
            parse_trade_msg(
                msg,
                *instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )
        },
        Data::Trade,
    )
}

/// Parses vector of OKX mark price messages into Nautilus mark price updates.
///
/// # Errors
///
/// Returns an error if any mark price message fails to parse.
pub fn parse_mark_price_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    parse_message_vec(
        data,
        |msg| parse_mark_price_msg(msg, *instrument_id, price_precision, ts_init),
        Data::MarkPriceUpdate,
    )
}

/// Parses vector of OKX index price messages into Nautilus index price updates.
///
/// # Errors
///
/// Returns an error if any index price message fails to parse.
pub fn parse_index_price_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    parse_message_vec(
        data,
        |msg| parse_index_price_msg(msg, *instrument_id, price_precision, ts_init),
        Data::IndexPriceUpdate,
    )
}

/// Parses vector of OKX funding rate messages into Nautilus funding rate updates.
/// Includes caching to filter out duplicate funding rates.
///
/// # Errors
///
/// Returns an error if any funding rate message fails to parse.
pub fn parse_funding_rate_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    ts_init: UnixNanos,
    funding_cache: &mut AHashMap<Ustr, (Ustr, u64)>,
) -> anyhow::Result<Vec<FundingRateUpdate>> {
    let msgs: Vec<OKXFundingRateMsg> = serde_json::from_value(data)?;

    let mut result = Vec::with_capacity(msgs.len());
    for msg in &msgs {
        let cache_key = (msg.funding_rate, msg.funding_time);

        if let Some(cached) = funding_cache.get(&msg.inst_id)
            && *cached == cache_key
        {
            continue; // Skip duplicate
        }

        // New or changed funding rate, update cache and parse
        funding_cache.insert(msg.inst_id, cache_key);
        let funding_rate = parse_funding_rate_msg(msg, *instrument_id, ts_init)?;
        result.push(funding_rate);
    }

    Ok(result)
}

/// Parses vector of OKX candle messages into Nautilus bars.
///
/// # Errors
///
/// Returns an error if candle messages cannot be deserialized or parsed.
pub fn parse_candle_msg_vec(
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    spec: BarSpecification,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let msgs: Vec<OKXCandleMsg> = serde_json::from_value(data)?;
    let bar_type = BarType::new(*instrument_id, spec, AggregationSource::External);
    let mut bars = Vec::with_capacity(msgs.len());

    for msg in msgs {
        // Only process completed candles to avoid duplicate/partial bars
        if msg.confirm == OKXCandleConfirm::Closed {
            let bar = parse_candle_msg(&msg, bar_type, price_precision, size_precision, ts_init)?;
            bars.push(Data::Bar(bar));
        }
    }

    Ok(bars)
}

/// Parses vector of OKX book messages into Nautilus depth10 updates.
///
/// # Errors
///
/// Returns an error if any book10 message fails to parse.
pub fn parse_book10_msg_vec(
    data: Vec<OKXBookMsg>,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Data>> {
    let mut depth10_updates = Vec::with_capacity(data.len());

    for msg in data {
        let depth10 = parse_book10_msg(
            &msg,
            *instrument_id,
            price_precision,
            size_precision,
            ts_init,
        )?;
        depth10_updates.push(Data::Depth10(Box::new(depth10)));
    }

    Ok(depth10_updates)
}

/// Parses an OKX book message into Nautilus order book deltas.
///
/// # Errors
///
/// Returns an error if bid or ask levels contain values that cannot be parsed.
pub fn parse_book_msg(
    msg: &OKXBookMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    action: &OKXBookAction,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let flags = if action == &OKXBookAction::Snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };
    let ts_event = parse_millisecond_timestamp(msg.ts);

    let mut deltas = Vec::with_capacity(msg.asks.len() + msg.bids.len());

    for bid in &msg.bids {
        let book_action = match action {
            OKXBookAction::Snapshot => BookAction::Add,
            _ => match bid.size.as_str() {
                "0" => BookAction::Delete,
                _ => BookAction::Update,
            },
        };
        let price = parse_price(&bid.price, price_precision)?;
        let size = parse_quantity(&bid.size, size_precision)?;
        let order_id = 0; // TBD
        let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
        let delta = OrderBookDelta::new(
            instrument_id,
            book_action,
            order,
            flags,
            msg.seq_id,
            ts_event,
            ts_init,
        );
        deltas.push(delta);
    }

    for ask in &msg.asks {
        let book_action = match action {
            OKXBookAction::Snapshot => BookAction::Add,
            _ => match ask.size.as_str() {
                "0" => BookAction::Delete,
                _ => BookAction::Update,
            },
        };
        let price = parse_price(&ask.price, price_precision)?;
        let size = parse_quantity(&ask.size, size_precision)?;
        let order_id = 0; // TBD
        let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
        let delta = OrderBookDelta::new(
            instrument_id,
            book_action,
            order,
            flags,
            msg.seq_id,
            ts_event,
            ts_init,
        );
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses an OKX book message into a Nautilus quote tick.
///
/// # Errors
///
/// Returns an error if any quote levels contain values that cannot be parsed.
pub fn parse_quote_msg(
    msg: &OKXBookMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let best_bid: &OrderBookEntry = &msg.bids[0];
    let best_ask: &OrderBookEntry = &msg.asks[0];

    let bid_price = parse_price(&best_bid.price, price_precision)?;
    let ask_price = parse_price(&best_ask.price, price_precision)?;
    let bid_size = parse_quantity(&best_bid.size, size_precision)?;
    let ask_size = parse_quantity(&best_ask.size, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

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

/// Parses an OKX book message into a Nautilus [`OrderBookDepth10`].
///
/// Converts order book data into a fixed-depth snapshot with top 10 levels for both sides.
///
/// # Errors
///
/// Returns an error if price or size fields cannot be parsed for any level.
pub fn parse_book10_msg(
    msg: &OKXBookMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDepth10> {
    // Initialize arrays - need to fill all 10 levels even if we have fewer
    let mut bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let mut asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
    let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];

    // Parse available bid levels (up to 10)
    let bid_len = msg.bids.len().min(DEPTH10_LEN);
    for (i, level) in msg.bids.iter().take(DEPTH10_LEN).enumerate() {
        let price = parse_price(&level.price, price_precision)?;
        let size = parse_quantity(&level.size, size_precision)?;
        let orders_count = level.orders_count.parse::<u32>().unwrap_or(1);

        let bid_order = BookOrder::new(OrderSide::Buy, price, size, 0);
        bids[i] = bid_order;
        bid_counts[i] = orders_count;
    }

    // Fill remaining bid slots with empty Buy orders (not NULL orders)
    for i in bid_len..DEPTH10_LEN {
        bids[i] = BookOrder::new(
            OrderSide::Buy,
            Price::zero(price_precision),
            Quantity::zero(size_precision),
            0,
        );
        bid_counts[i] = 0;
    }

    // Parse available ask levels (up to 10)
    let ask_len = msg.asks.len().min(DEPTH10_LEN);
    for (i, level) in msg.asks.iter().take(DEPTH10_LEN).enumerate() {
        let price = parse_price(&level.price, price_precision)?;
        let size = parse_quantity(&level.size, size_precision)?;
        let orders_count = level.orders_count.parse::<u32>().unwrap_or(1);

        let ask_order = BookOrder::new(OrderSide::Sell, price, size, 0);
        asks[i] = ask_order;
        ask_counts[i] = orders_count;
    }

    // Fill remaining ask slots with empty Sell orders (not NULL orders)
    for i in ask_len..DEPTH10_LEN {
        asks[i] = BookOrder::new(
            OrderSide::Sell,
            Price::zero(price_precision),
            Quantity::zero(size_precision),
            0,
        );
        ask_counts[i] = 0;
    }

    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        msg.seq_id, // Use sequence ID for OKX L2 books
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX ticker message into a Nautilus quote tick.
///
/// # Errors
///
/// Returns an error if bid or ask values cannot be parsed from the message.
pub fn parse_ticker_msg(
    msg: &OKXTickerMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = parse_price(&msg.bid_px, price_precision)?;
    let ask_price = parse_price(&msg.ask_px, price_precision)?;
    let bid_size = parse_quantity(&msg.bid_sz, size_precision)?;
    let ask_size = parse_quantity(&msg.ask_sz, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

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

/// Parses an OKX trade message into a Nautilus trade tick.
///
/// # Errors
///
/// Returns an error if trade prices or sizes cannot be parsed.
pub fn parse_trade_msg(
    msg: &OKXTradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&msg.px, price_precision)?;
    let size = parse_quantity(&msg.sz, size_precision)?;
    let aggressor_side: AggressorSide = msg.side.into();
    let trade_id = TradeId::new(&msg.trade_id);
    let ts_event = parse_millisecond_timestamp(msg.ts);

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

/// Parses an OKX mark price message into a Nautilus mark price update.
///
/// # Errors
///
/// Returns an error if the mark price fails to parse.
pub fn parse_mark_price_msg(
    msg: &OKXMarkPriceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let price = parse_price(&msg.mark_px, price_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(MarkPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX index price message into a Nautilus index price update.
///
/// # Errors
///
/// Returns an error if the index price fails to parse.
pub fn parse_index_price_msg(
    msg: &OKXIndexPriceMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let price = parse_price(&msg.idx_px, price_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(IndexPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX candle message into a Nautilus bar.
///
/// # Errors
///
/// Returns an error if candle price or volume fields cannot be parsed.
pub fn parse_candle_msg(
    msg: &OKXCandleMsg,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&msg.o, price_precision)?;
    let high = parse_price(&msg.h, price_precision)?;
    let low = parse_price(&msg.l, price_precision)?;
    let close = parse_price(&msg.c, price_precision)?;
    let volume = parse_quantity(&msg.vol, size_precision)?;
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses vector of OKX order messages into Nautilus execution reports.
///
/// # Errors
///
/// Returns an error if any contained order messages cannot be parsed.
pub fn parse_order_msg_vec(
    data: Vec<OKXOrderMsg>,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &AHashMap<Ustr, Money>,
    filled_qty_cache: &AHashMap<Ustr, Quantity>,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<ExecutionReport>> {
    let mut order_reports = Vec::with_capacity(data.len());

    for msg in data {
        match parse_order_msg(
            &msg,
            account_id,
            instruments,
            fee_cache,
            filled_qty_cache,
            ts_init,
        ) {
            Ok(report) => order_reports.push(report),
            Err(e) => tracing::error!("Failed to parse execution report from message: {e}"),
        }
    }

    Ok(order_reports)
}

/// Checks if acc_fill_sz has increased compared to the previous filled quantity.
fn has_acc_fill_sz_increased(
    acc_fill_sz: &Option<String>,
    previous_filled_qty: Option<Quantity>,
    size_precision: u8,
) -> bool {
    if let Some(acc_str) = acc_fill_sz {
        if acc_str.is_empty() || acc_str == "0" {
            return false;
        }
        if let Ok(current_filled) = parse_quantity(acc_str, size_precision) {
            if let Some(prev_qty) = previous_filled_qty {
                return current_filled > prev_qty;
            }
            return !current_filled.is_zero();
        }
    }
    false
}

/// Parses a single OKX order message into an [`ExecutionReport`].
///
/// # Errors
///
/// Returns an error if the instrument cannot be found or if parsing the
/// underlying order payload fails.
pub fn parse_order_msg(
    msg: &OKXOrderMsg,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &AHashMap<Ustr, Money>,
    filled_qty_cache: &AHashMap<Ustr, Quantity>,
    ts_init: UnixNanos,
) -> anyhow::Result<ExecutionReport> {
    let instrument = instruments
        .get(&msg.inst_id)
        .ok_or_else(|| anyhow::anyhow!("No instrument found for inst_id: {}", msg.inst_id))?;

    let previous_fee = fee_cache.get(&msg.ord_id).copied();
    let previous_filled_qty = filled_qty_cache.get(&msg.ord_id).copied();

    let has_new_fill = (!msg.fill_sz.is_empty() && msg.fill_sz != "0")
        || !msg.trade_id.is_empty()
        || has_acc_fill_sz_increased(
            &msg.acc_fill_sz,
            previous_filled_qty,
            instrument.size_precision(),
        );

    match msg.state {
        OKXOrderStatus::Filled | OKXOrderStatus::PartiallyFilled if has_new_fill => {
            parse_fill_report(
                msg,
                instrument,
                account_id,
                previous_fee,
                previous_filled_qty,
                ts_init,
            )
            .map(ExecutionReport::Fill)
        }
        _ => parse_order_status_report(msg, instrument, account_id, ts_init)
            .map(ExecutionReport::Order),
    }
}

/// Parses an OKX algo order message into a Nautilus execution report.
///
/// # Errors
///
/// Returns an error if the instrument cannot be found or if message fields
/// fail to parse.
pub fn parse_algo_order_msg(
    msg: OKXAlgoOrderMsg,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> anyhow::Result<ExecutionReport> {
    let inst = instruments
        .get(&msg.inst_id)
        .ok_or_else(|| anyhow::anyhow!("No instrument found for inst_id: {}", msg.inst_id))?;

    // Algo orders primarily return status reports (not fills since they haven't been triggered yet)
    parse_algo_order_status_report(&msg, inst, account_id, ts_init).map(ExecutionReport::Order)
}

/// Parses an OKX algo order message into a Nautilus order status report.
///
/// # Errors
///
/// Returns an error if any order identifiers or numeric fields cannot be
/// parsed.
pub fn parse_algo_order_status_report(
    msg: &OKXAlgoOrderMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    // For algo orders, use algo_cl_ord_id if cl_ord_id is empty
    let client_order_id = if msg.cl_ord_id.is_empty() {
        parse_client_order_id(&msg.algo_cl_ord_id)
    } else {
        parse_client_order_id(&msg.cl_ord_id)
    };

    // For algo orders that haven't triggered, ord_id will be empty, use algo_id instead
    let venue_order_id = if msg.ord_id.is_empty() {
        VenueOrderId::new(msg.algo_id.as_str())
    } else {
        VenueOrderId::new(msg.ord_id.as_str())
    };

    let order_side: OrderSide = msg.side.into();

    // Determine order type based on ord_px for conditional/stop orders
    let order_type = if is_market_price(&msg.ord_px) {
        OrderType::StopMarket
    } else {
        OrderType::StopLimit
    };

    let status: OrderStatus = msg.state.into();

    let quantity = parse_quantity(msg.sz.as_str(), instrument.size_precision())?;

    // For algo orders, actual_sz represents filled quantity (if any)
    let filled_qty = if msg.actual_sz.is_empty() || msg.actual_sz == "0" {
        Quantity::zero(instrument.size_precision())
    } else {
        parse_quantity(msg.actual_sz.as_str(), instrument.size_precision())?
    };

    let trigger_px = parse_price(msg.trigger_px.as_str(), instrument.price_precision())?;

    // Parse limit price if it exists (not -1)
    let price = if msg.ord_px != "-1" {
        Some(parse_price(
            msg.ord_px.as_str(),
            instrument.price_precision(),
        )?)
    } else {
        None
    };

    let trigger_type = match msg.trigger_px_type {
        OKXTriggerType::Last => TriggerType::LastPrice,
        OKXTriggerType::Mark => TriggerType::MarkPrice,
        OKXTriggerType::Index => TriggerType::IndexPrice,
        OKXTriggerType::None => TriggerType::Default,
    };

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        TimeInForce::Gtc, // Algo orders are typically GTC
        status,
        quantity,
        filled_qty,
        msg.c_time.into(), // ts_accepted
        msg.u_time.into(), // ts_last
        ts_init,
        None, // report_id - auto-generated
    );

    report.trigger_price = Some(trigger_px);
    report.trigger_type = Some(trigger_type);

    if let Some(limit_price) = price {
        report.price = Some(limit_price);
    }

    Ok(report)
}

/// Parses an OKX order message into a Nautilus order status report.
///
/// # Errors
///
/// Returns an error if order metadata or numeric values cannot be parsed.
pub fn parse_order_status_report(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let client_order_id = parse_client_order_id(&msg.cl_ord_id);
    let venue_order_id = VenueOrderId::new(msg.ord_id);
    let order_side: OrderSide = msg.side.into();

    let okx_order_type = msg.ord_type;

    // Determine order type based on presence of limit price for certain OKX order types
    let order_type = match okx_order_type {
        // Trigger orders: check if they have a price
        OKXOrderType::Trigger => {
            if is_market_price(&msg.px) {
                OrderType::StopMarket
            } else {
                OrderType::StopLimit
            }
        }
        // FOK/IOC orders: check if they have a price
        // Without a price, they're market orders with TIF
        // With a price, they're limit orders with TIF
        OKXOrderType::Fok | OKXOrderType::Ioc | OKXOrderType::OptimalLimitIoc => {
            if is_market_price(&msg.px) {
                OrderType::Market
            } else {
                OrderType::Limit
            }
        }
        // All other order types use standard mapping
        _ => msg.ord_type.into(),
    };
    let order_status: OrderStatus = msg.state.into();

    let time_in_force = match okx_order_type {
        OKXOrderType::Fok => TimeInForce::Fok,
        OKXOrderType::Ioc | OKXOrderType::OptimalLimitIoc => TimeInForce::Ioc,
        _ => TimeInForce::Gtc,
    };

    let size_precision = instrument.size_precision();

    // Parse quantities based on target currency
    // OKX always returns acc_fill_sz in base currency, but sz depends on tgt_ccy

    // Determine if this is a quote-quantity order
    // Method 1: Explicit tgt_ccy field set to QuoteCcy
    let is_quote_qty_explicit = msg.tgt_ccy == Some(OKXTargetCurrency::QuoteCcy);

    // Method 2: Use OKX defaults when tgt_ccy is None (old orders or missing field)
    // OKX API defaults for SPOT market orders: BUY orders use quote_ccy, SELL orders use base_ccy
    // Note: tgtCcy only applies to SPOT market orders (not limit orders)
    // For limit orders, sz is always in base currency regardless of side
    let is_quote_qty_heuristic = msg.tgt_ccy.is_none()
        && (msg.inst_type == OKXInstrumentType::Spot || msg.inst_type == OKXInstrumentType::Margin)
        && msg.side == OKXSide::Buy
        && msg.ord_type == OKXOrderType::Market;

    let (quantity, filled_qty) = if is_quote_qty_explicit || is_quote_qty_heuristic {
        // Quote-quantity order: sz is in quote currency, need to convert to base
        let sz_quote = msg.sz.parse::<f64>().map_err(|e| {
            anyhow::anyhow!("Failed to parse sz='{}' as quote quantity: {}", msg.sz, e)
        })?;

        // Determine the price to use for conversion
        // Priority: 1) limit price (px) for limit orders, 2) avg_px for market orders
        let conversion_price = if !is_market_price(&msg.px) {
            // Limit order: use the limit price (msg.px)
            msg.px
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse px='{}': {}", msg.px, e))?
        } else if !msg.avg_px.is_empty() && msg.avg_px != "0" {
            // Market order with fills: use average fill price
            msg.avg_px
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse avg_px='{}': {}", msg.avg_px, e))?
        } else {
            0.0
        };

        // Convert quote quantity to base: quantity_base = sz_quote / price
        let quantity_base = if conversion_price > 0.0 {
            Quantity::new(sz_quote / conversion_price, size_precision)
        } else {
            // No price available, can't convert - use sz as-is temporarily
            // This will be corrected once the order gets filled and price is available
            parse_quantity(&msg.sz, size_precision)?
        };

        let filled_qty =
            parse_quantity(&msg.acc_fill_sz.clone().unwrap_or_default(), size_precision)?;

        (quantity_base, filled_qty)
    } else {
        // Base-quantity order: both sz and acc_fill_sz are in base currency
        let quantity = parse_quantity(&msg.sz, size_precision)?;
        let filled_qty =
            parse_quantity(&msg.acc_fill_sz.clone().unwrap_or_default(), size_precision)?;

        (quantity, filled_qty)
    };

    // For quote-quantity orders marked as FILLED, adjust quantity to match filled_qty
    // to avoid precision mismatches from quote-to-base conversion
    let (quantity, filled_qty) = if (is_quote_qty_explicit || is_quote_qty_heuristic)
        && msg.state == OKXOrderStatus::Filled
        && filled_qty.is_positive()
    {
        (filled_qty, filled_qty)
    } else {
        (quantity, filled_qty)
    };

    let ts_accepted = parse_millisecond_timestamp(msg.c_time);
    let ts_last = parse_millisecond_timestamp(msg.u_time);

    let is_liquidation = matches!(
        msg.category,
        OKXOrderCategory::FullLiquidation | OKXOrderCategory::PartialLiquidation
    );

    let is_adl = msg.category == OKXOrderCategory::Adl;

    if is_liquidation {
        tracing::warn!(
            order_id = msg.ord_id.as_str(),
            category = ?msg.category,
            inst_id = msg.inst_id.as_str(),
            state = ?msg.state,
            "Liquidation order status update"
        );
    }

    if is_adl {
        tracing::warn!(
            order_id = msg.ord_id.as_str(),
            inst_id = msg.inst_id.as_str(),
            state = ?msg.state,
            "ADL (Auto-Deleveraging) order status update"
        );
    }

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_init,
        ts_last,
        None, // Generate UUID4 automatically
    );

    let price_precision = instrument.price_precision();

    if okx_order_type == OKXOrderType::Trigger {
        // For triggered orders coming through regular orders channel,
        // set the price if it's a stop-limit order
        if !is_market_price(&msg.px)
            && let Ok(price) = parse_price(&msg.px, price_precision)
        {
            report = report.with_price(price);
        }
    } else {
        // For regular orders, use px field
        if !is_market_price(&msg.px)
            && let Ok(price) = parse_price(&msg.px, price_precision)
        {
            report = report.with_price(price);
        }
    }

    if !msg.avg_px.is_empty()
        && let Ok(avg_px) = msg.avg_px.parse::<f64>()
    {
        report = report.with_avg_px(avg_px);
    }

    if matches!(
        msg.ord_type,
        OKXOrderType::PostOnly | OKXOrderType::MmpAndPostOnly
    ) || matches!(
        msg.cancel_source.as_deref(),
        Some(source) if source == OKX_POST_ONLY_CANCEL_SOURCE
    ) || matches!(
        msg.cancel_source_reason.as_deref(),
        Some(reason) if reason.contains("POST_ONLY")
    ) {
        report = report.with_post_only(true);
    }

    if msg.reduce_only == "true" {
        report = report.with_reduce_only(true);
    }

    if let Some(reason) = msg
        .cancel_source_reason
        .as_ref()
        .filter(|reason| !reason.is_empty())
    {
        report = report.with_cancel_reason(reason.clone());
    } else if let Some(source) = msg
        .cancel_source
        .as_ref()
        .filter(|source| !source.is_empty())
    {
        let reason = if source == OKX_POST_ONLY_CANCEL_SOURCE {
            OKX_POST_ONLY_CANCEL_REASON.to_string()
        } else {
            format!("cancel_source={source}")
        };
        report = report.with_cancel_reason(reason);
    }

    Ok(report)
}

/// Parses an OKX order message into a Nautilus fill report.
///
/// # Errors
///
/// Returns an error if order quantities, prices, or fees cannot be parsed.
pub fn parse_fill_report(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    previous_fee: Option<Money>,
    previous_filled_qty: Option<Quantity>,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let client_order_id = parse_client_order_id(&msg.cl_ord_id);
    let venue_order_id = VenueOrderId::new(msg.ord_id);

    // TODO: Extract to dedicated function:
    // OKX may not provide a trade_id, so generate a UUID4 as fallback
    let trade_id = if msg.trade_id.is_empty() {
        TradeId::from(UUID4::new().to_string().as_str())
    } else {
        TradeId::from(msg.trade_id.as_str())
    };

    let order_side: OrderSide = msg.side.into();

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price_str = if !msg.fill_px.is_empty() {
        &msg.fill_px
    } else if !msg.avg_px.is_empty() {
        &msg.avg_px
    } else {
        &msg.px
    };
    let last_px = parse_price(price_str, price_precision).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse price (fill_px='{}', avg_px='{}', px='{}'): {}",
            msg.fill_px,
            msg.avg_px,
            msg.px,
            e
        )
    })?;

    // OKX provides fillSz (incremental fill) or accFillSz (cumulative total)
    // If fillSz is provided, use it directly as the incremental fill quantity
    let last_qty = if !msg.fill_sz.is_empty() && msg.fill_sz != "0" {
        parse_quantity(&msg.fill_sz, size_precision)
            .map_err(|e| anyhow::anyhow!("Failed to parse fill_sz='{}': {e}", msg.fill_sz,))?
    } else if let Some(ref acc_fill_sz) = msg.acc_fill_sz {
        // If fillSz is missing but accFillSz is available, calculate incremental fill
        if !acc_fill_sz.is_empty() && acc_fill_sz != "0" {
            let current_filled = parse_quantity(acc_fill_sz, size_precision).map_err(|e| {
                anyhow::anyhow!("Failed to parse acc_fill_sz='{}': {e}", acc_fill_sz,)
            })?;

            // Calculate incremental fill as: current_total - previous_total
            if let Some(prev_qty) = previous_filled_qty {
                let incremental = current_filled - prev_qty;
                if incremental.is_zero() {
                    anyhow::bail!(
                        "Incremental fill quantity is zero (acc_fill_sz='{}', previous_filled_qty={})",
                        acc_fill_sz,
                        prev_qty
                    );
                }
                incremental
            } else {
                // First fill, use accumulated as incremental
                current_filled
            }
        } else {
            anyhow::bail!(
                "Cannot determine fill quantity: fill_sz is empty/zero and acc_fill_sz is empty/zero"
            );
        }
    } else {
        anyhow::bail!(
            "Cannot determine fill quantity: fill_sz='{}' and acc_fill_sz is None",
            msg.fill_sz
        );
    };

    let fee_str = msg.fee.as_deref().unwrap_or("0");
    let fee_value = fee_str
        .parse::<f64>()
        .map_err(|e| anyhow::anyhow!("Failed to parse fee '{}': {}", fee_str, e))?;

    let fee_currency = parse_fee_currency(msg.fee_ccy.as_str(), fee_value, || {
        format!("fill report for inst_id={}", msg.inst_id)
    });

    // OKX sends fees as negative numbers (e.g., "-2.5" for a $2.5 charge), parse_fee negates to positive
    let total_fee = parse_fee(msg.fee.as_deref(), fee_currency)
        .map_err(|e| anyhow::anyhow!("Failed to parse fee={:?}: {}", msg.fee, e))?;

    // OKX sends cumulative fees, so we subtract the previous total to get this fill's fee
    let commission = if let Some(previous_fee) = previous_fee {
        let incremental = total_fee - previous_fee;

        if incremental < Money::zero(fee_currency) {
            tracing::debug!(
                order_id = msg.ord_id.as_str(),
                total_fee = %total_fee,
                previous_fee = %previous_fee,
                incremental = %incremental,
                "Negative incremental fee detected - likely a maker rebate or fee refund"
            );
        }

        // Skip corruption check when previous is negative (rebate), as transitions from
        // rebate to charge legitimately have incremental > total (e.g., -1 → +2 gives +3)
        if previous_fee >= Money::zero(fee_currency)
            && total_fee > Money::zero(fee_currency)
            && incremental > total_fee
        {
            tracing::error!(
                order_id = msg.ord_id.as_str(),
                total_fee = %total_fee,
                previous_fee = %previous_fee,
                incremental = %incremental,
                "Incremental fee exceeds total fee - likely fee cache corruption, using total fee as fallback"
            );
            total_fee
        } else {
            incremental
        }
    } else {
        total_fee
    };

    let liquidity_side: LiquiditySide = msg.exec_type.into();
    let ts_event = parse_millisecond_timestamp(msg.fill_time);

    let is_liquidation = matches!(
        msg.category,
        OKXOrderCategory::FullLiquidation | OKXOrderCategory::PartialLiquidation
    );

    let is_adl = msg.category == OKXOrderCategory::Adl;

    if is_liquidation {
        tracing::warn!(
            order_id = msg.ord_id.as_str(),
            category = ?msg.category,
            inst_id = msg.inst_id.as_str(),
            side = ?msg.side,
            fill_sz = %msg.fill_sz,
            fill_px = %msg.fill_px,
            "Liquidation order detected"
        );
    }

    if is_adl {
        tracing::warn!(
            order_id = msg.ord_id.as_str(),
            inst_id = msg.inst_id.as_str(),
            side = ?msg.side,
            fill_sz = %msg.fill_sz,
            fill_px = %msg.fill_px,
            "ADL (Auto-Deleveraging) order detected"
        );
    }

    let report = FillReport::new(
        account_id,
        instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        None, // Generate UUID4 automatically
    );

    Ok(report)
}

/// Parses OKX WebSocket message payloads into Nautilus data structures.
///
/// # Errors
///
/// Returns an error if the payload cannot be deserialized or if downstream
/// parsing routines fail.
///
/// # Panics
///
/// Panics only in the case where `okx_channel_to_bar_spec(channel)` returns
/// `None` after a prior `is_some` check – an unreachable scenario indicating a
/// logic error.
#[allow(clippy::too_many_arguments)]
pub fn parse_ws_message_data(
    channel: &OKXWsChannel,
    data: serde_json::Value,
    instrument_id: &InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
    funding_cache: &mut AHashMap<Ustr, (Ustr, u64)>,
    instruments_cache: &AHashMap<Ustr, InstrumentAny>,
) -> anyhow::Result<Option<NautilusWsMessage>> {
    match channel {
        OKXWsChannel::Instruments => {
            if let Ok(msg) = serde_json::from_value::<OKXInstrument>(data) {
                // Look up cached instrument to extract existing fees
                let (margin_init, margin_maint, maker_fee, taker_fee) =
                    instruments_cache.get(&Ustr::from(&msg.inst_id)).map_or(
                        (None, None, None, None),
                        extract_fees_from_cached_instrument,
                    );

                match parse_instrument_any(
                    &msg,
                    margin_init,
                    margin_maint,
                    maker_fee,
                    taker_fee,
                    ts_init,
                )? {
                    Some(inst_any) => Ok(Some(NautilusWsMessage::Instrument(Box::new(inst_any)))),
                    None => {
                        tracing::warn!("Empty instrument payload: {:?}", msg);
                        Ok(None)
                    }
                }
            } else {
                anyhow::bail!("Failed to deserialize instrument payload")
            }
        }
        OKXWsChannel::BboTbt => {
            let data_vec = parse_quote_msg_vec(
                data,
                instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::Tickers => {
            let data_vec = parse_ticker_msg_vec(
                data,
                instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::Trades => {
            let data_vec = parse_trade_msg_vec(
                data,
                instrument_id,
                price_precision,
                size_precision,
                ts_init,
            )?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::MarkPrice => {
            let data_vec = parse_mark_price_msg_vec(data, instrument_id, price_precision, ts_init)?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::IndexTickers => {
            let data_vec =
                parse_index_price_msg_vec(data, instrument_id, price_precision, ts_init)?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::FundingRate => {
            let data_vec = parse_funding_rate_msg_vec(data, instrument_id, ts_init, funding_cache)?;
            Ok(Some(NautilusWsMessage::FundingRates(data_vec)))
        }
        channel if okx_channel_to_bar_spec(channel).is_some() => {
            let bar_spec = okx_channel_to_bar_spec(channel).expect("bar_spec checked above");
            let data_vec = parse_candle_msg_vec(
                data,
                instrument_id,
                price_precision,
                size_precision,
                bar_spec,
                ts_init,
            )?;
            Ok(Some(NautilusWsMessage::Data(data_vec)))
        }
        OKXWsChannel::Books
        | OKXWsChannel::BooksTbt
        | OKXWsChannel::Books5
        | OKXWsChannel::Books50Tbt => {
            if let Ok(book_msgs) = serde_json::from_value::<Vec<OKXBookMsg>>(data) {
                let data_vec = parse_book10_msg_vec(
                    book_msgs,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                )?;
                Ok(Some(NautilusWsMessage::Data(data_vec)))
            } else {
                anyhow::bail!("Failed to deserialize Books channel data as Vec<OKXBookMsg>")
            }
        }
        _ => {
            tracing::warn!("Unsupported channel for message parsing: {channel:?}");
            Ok(None)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        data::bar::BAR_SPEC_1_DAY_LAST,
        identifiers::{ClientOrderId, Symbol},
        instruments::CryptoPerpetual,
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;
    use crate::{
        OKXPositionSide,
        common::{
            enums::{OKXExecType, OKXInstrumentType, OKXOrderType, OKXSide, OKXTradeMode},
            parse::parse_account_state,
            testing::load_test_json,
        },
        http::models::OKXAccount,
        websocket::messages::{OKXWebSocketArg, OKXWebSocketEvent},
    };

    fn create_stub_instrument() -> CryptoPerpetual {
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn create_stub_order_msg(
        fill_sz: &str,
        acc_fill_sz: Option<String>,
        order_id: &str,
        trade_id: &str,
    ) -> OKXOrderMsg {
        OKXOrderMsg {
            acc_fill_sz,
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_1".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-1.0".to_string()),
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: fill_sz.to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from(order_id),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::PartiallyFilled,
            exec_type: OKXExecType::Taker,
            sz: "0.03".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: trade_id.to_string(),
            u_time: 1746947317402,
        }
    }

    #[rstest]
    fn test_parse_books_snapshot() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (okx_books, action): (Vec<OKXBookMsg>, OKXBookAction) = match msg {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected a `BookData` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let deltas = parse_book_msg(
            &okx_books[0],
            instrument_id,
            2,
            1,
            &action,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 16);
        assert_eq!(deltas.flags, 32);
        assert_eq!(deltas.sequence, 123456);
        assert_eq!(deltas.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(deltas.ts_init, UnixNanos::default());

        // Verify some individual deltas are parsed correctly
        assert!(!deltas.deltas.is_empty());
        // Snapshot should have both bid and ask deltas
        let bid_deltas: Vec<_> = deltas
            .deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Buy)
            .collect();
        let ask_deltas: Vec<_> = deltas
            .deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Sell)
            .collect();
        assert!(!bid_deltas.is_empty());
        assert!(!ask_deltas.is_empty());
    }

    #[rstest]
    fn test_parse_books_update() {
        let json_data = load_test_json("ws_books_update.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let (okx_books, action): (Vec<OKXBookMsg>, OKXBookAction) = match msg {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected a `BookData` variant"),
        };

        let deltas = parse_book_msg(
            &okx_books[0],
            instrument_id,
            2,
            1,
            &action,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 16);
        assert_eq!(deltas.flags, 0);
        assert_eq!(deltas.sequence, 123457);
        assert_eq!(deltas.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(deltas.ts_init, UnixNanos::default());

        // Verify some individual deltas are parsed correctly
        assert!(!deltas.deltas.is_empty());
        // Update should also have both bid and ask deltas
        let bid_deltas: Vec<_> = deltas
            .deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Buy)
            .collect();
        let ask_deltas: Vec<_> = deltas
            .deltas
            .iter()
            .filter(|d| d.order.side == OrderSide::Sell)
            .collect();
        assert!(!bid_deltas.is_empty());
        assert!(!ask_deltas.is_empty());
    }

    #[rstest]
    fn test_parse_tickers() {
        let json_data = load_test_json("ws_tickers.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_tickers: Vec<OKXTickerMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trade =
            parse_ticker_msg(&okx_tickers[0], instrument_id, 2, 1, UnixNanos::default()).unwrap();

        assert_eq!(trade.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(trade.bid_price, Price::from("8888.88"));
        assert_eq!(trade.ask_price, Price::from("9999.99"));
        assert_eq!(trade.bid_size, Quantity::from(5));
        assert_eq!(trade.ask_size, Quantity::from(11));
        assert_eq!(trade.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(trade.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_quotes() {
        let json_data = load_test_json("ws_bbo_tbt.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_quotes: Vec<OKXBookMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        let quote =
            parse_quote_msg(&okx_quotes[0], instrument_id, 2, 1, UnixNanos::default()).unwrap();

        assert_eq!(quote.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(quote.bid_price, Price::from("8476.97"));
        assert_eq!(quote.ask_price, Price::from("8476.98"));
        assert_eq!(quote.bid_size, Quantity::from(256));
        assert_eq!(quote.ask_size, Quantity::from(415));
        assert_eq!(quote.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(quote.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("ws_trades.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_trades: Vec<OKXTradeMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trade =
            parse_trade_msg(&okx_trades[0], instrument_id, 1, 8, UnixNanos::default()).unwrap();

        assert_eq!(trade.instrument_id, InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(trade.price, Price::from("42219.9"));
        assert_eq!(trade.size, Quantity::from("0.12060306"));
        assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
        assert_eq!(trade.trade_id, TradeId::from("130639474"));
        assert_eq!(trade.ts_event, UnixNanos::from(1630048897897000000));
        assert_eq!(trade.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_candle() {
        let json_data = load_test_json("ws_candle.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let okx_candles: Vec<OKXCandleMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let bar_type = BarType::new(
            instrument_id,
            BAR_SPEC_1_DAY_LAST,
            AggregationSource::External,
        );
        let bar = parse_candle_msg(&okx_candles[0], bar_type, 2, 0, UnixNanos::default()).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("8533.02"));
        assert_eq!(bar.high, Price::from("8553.74"));
        assert_eq!(bar.low, Price::from("8527.17"));
        assert_eq!(bar.close, Price::from("8548.26"));
        assert_eq!(bar.volume, Quantity::from(45247));
        assert_eq!(bar.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(bar.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_funding_rate() {
        let json_data = load_test_json("ws_funding_rate.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();

        let okx_funding_rates: Vec<crate::websocket::messages::OKXFundingRateMsg> = match msg {
            OKXWebSocketEvent::Data { data, .. } => serde_json::from_value(data).unwrap(),
            _ => panic!("Expected a `Data` variant"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let funding_rate =
            parse_funding_rate_msg(&okx_funding_rates[0], instrument_id, UnixNanos::default())
                .unwrap();

        assert_eq!(funding_rate.instrument_id, instrument_id);
        assert_eq!(funding_rate.rate, Decimal::new(1, 4));
        assert_eq!(
            funding_rate.next_funding_ns,
            Some(UnixNanos::from(1744590349506000000))
        );
        assert_eq!(funding_rate.ts_event, UnixNanos::from(1744590349506000000));
        assert_eq!(funding_rate.ts_init, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_book_vec() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (msgs, action): (Vec<OKXBookMsg>, OKXBookAction) = match event {
            OKXWebSocketEvent::BookData { data, action, .. } => (data, action),
            _ => panic!("Expected BookData"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let deltas_vec =
            parse_book_msg_vec(msgs, &instrument_id, 8, 1, action, UnixNanos::default()).unwrap();

        assert_eq!(deltas_vec.len(), 1);

        if let Data::Deltas(d) = &deltas_vec[0] {
            assert_eq!(d.sequence, 123456);
        } else {
            panic!("Expected Deltas");
        }
    }

    #[rstest]
    fn test_parse_ticker_vec() {
        let json_data = load_test_json("ws_tickers.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let quotes_vec =
            parse_ticker_msg_vec(data_val, &instrument_id, 8, 1, UnixNanos::default()).unwrap();

        assert_eq!(quotes_vec.len(), 1);

        if let Data::Quote(q) = &quotes_vec[0] {
            assert_eq!(q.bid_price, Price::from("8888.88000000"));
            assert_eq!(q.ask_price, Price::from("9999.99"));
        } else {
            panic!("Expected Quote");
        }
    }

    #[rstest]
    fn test_parse_trade_vec() {
        let json_data = load_test_json("ws_trades.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trades_vec =
            parse_trade_msg_vec(data_val, &instrument_id, 8, 1, UnixNanos::default()).unwrap();

        assert_eq!(trades_vec.len(), 1);

        if let Data::Trade(t) = &trades_vec[0] {
            assert_eq!(t.trade_id, TradeId::new("130639474"));
        } else {
            panic!("Expected Trade");
        }
    }

    #[rstest]
    fn test_parse_candle_vec() {
        let json_data = load_test_json("ws_candle.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let data_val: serde_json::Value = match event {
            OKXWebSocketEvent::Data { data, .. } => data,
            _ => panic!("Expected Data"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let bars_vec = parse_candle_msg_vec(
            data_val,
            &instrument_id,
            2,
            1,
            BAR_SPEC_1_DAY_LAST,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(bars_vec.len(), 1);

        if let Data::Bar(b) = &bars_vec[0] {
            assert_eq!(b.open, Price::from("8533.02"));
        } else {
            panic!("Expected Bar");
        }
    }

    #[rstest]
    fn test_parse_book_message() {
        let json_data = load_test_json("ws_bbo_tbt.json");
        let msg: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let (okx_books, arg): (Vec<OKXBookMsg>, OKXWebSocketArg) = match msg {
            OKXWebSocketEvent::Data { data, arg, .. } => {
                (serde_json::from_value(data).unwrap(), arg)
            }
            _ => panic!("Expected a `Data` variant"),
        };

        assert_eq!(arg.channel, OKXWsChannel::BboTbt);
        assert_eq!(arg.inst_id.as_ref().unwrap(), &Ustr::from("BTC-USDT"));
        assert_eq!(arg.inst_type, None);
        assert_eq!(okx_books.len(), 1);

        let book_msg = &okx_books[0];

        // Check asks
        assert_eq!(book_msg.asks.len(), 1);
        let ask = &book_msg.asks[0];
        assert_eq!(ask.price, "8476.98");
        assert_eq!(ask.size, "415");
        assert_eq!(ask.liquidated_orders_count, "0");
        assert_eq!(ask.orders_count, "13");

        // Check bids
        assert_eq!(book_msg.bids.len(), 1);
        let bid = &book_msg.bids[0];
        assert_eq!(bid.price, "8476.97");
        assert_eq!(bid.size, "256");
        assert_eq!(bid.liquidated_orders_count, "0");
        assert_eq!(bid.orders_count, "12");
        assert_eq!(book_msg.ts, 1597026383085);
        assert_eq!(book_msg.seq_id, 123456);
        assert_eq!(book_msg.checksum, None);
        assert_eq!(book_msg.prev_seq_id, None);
    }

    #[rstest]
    fn test_parse_ws_account_message() {
        let json_data = load_test_json("ws_account.json");
        let accounts: Vec<OKXAccount> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(accounts.len(), 1);
        let account = &accounts[0];

        assert_eq!(account.total_eq, "100.56089404807182");
        assert_eq!(account.details.len(), 3);

        let usdt_detail = &account.details[0];
        assert_eq!(usdt_detail.ccy, "USDT");
        assert_eq!(usdt_detail.avail_bal, "100.52768569797846");
        assert_eq!(usdt_detail.cash_bal, "100.52768569797846");

        let btc_detail = &account.details[1];
        assert_eq!(btc_detail.ccy, "BTC");
        assert_eq!(btc_detail.avail_bal, "0.0000000051");

        let eth_detail = &account.details[2];
        assert_eq!(eth_detail.ccy, "ETH");
        assert_eq!(eth_detail.avail_bal, "0.000000185");

        let account_id = AccountId::new("OKX-001");
        let ts_init = nautilus_core::nanos::UnixNanos::default();
        let account_state = parse_account_state(account, account_id, ts_init);

        assert!(account_state.is_ok());
        let state = account_state.unwrap();
        assert_eq!(state.account_id, account_id);
        assert_eq!(state.balances.len(), 3);
    }

    #[rstest]
    fn test_parse_order_msg() {
        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();

        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create a mock instrument for testing
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None, // multiplier
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            UnixNanos::default(),
            UnixNanos::default(),
        );

        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let ts_init = UnixNanos::default();
        let fee_cache = AHashMap::new();
        let filled_qty_cache = AHashMap::new();

        let result = parse_order_msg_vec(
            data,
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            ts_init,
        );

        assert!(result.is_ok());
        let order_reports = result.unwrap();
        assert_eq!(order_reports.len(), 1);

        // Verify the parsed order report
        let report = &order_reports[0];

        if let ExecutionReport::Fill(fill_report) = report {
            assert_eq!(fill_report.account_id, account_id);
            assert_eq!(fill_report.instrument_id, instrument_id);
            assert_eq!(
                fill_report.client_order_id,
                Some(ClientOrderId::new("001BTCUSDT20250106001"))
            );
            assert_eq!(
                fill_report.venue_order_id,
                VenueOrderId::new("2497956918703120384")
            );
            assert_eq!(fill_report.trade_id, TradeId::from("1518905529"));
            assert_eq!(fill_report.order_side, OrderSide::Buy);
            assert_eq!(fill_report.last_px, Price::from("103698.90"));
            assert_eq!(fill_report.last_qty, Quantity::from("0.03000000"));
            assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
        } else {
            panic!("Expected Fill report for filled order");
        }
    }

    #[rstest]
    fn test_parse_order_status_report() {
        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();
        let order_msg = &data[0];

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let ts_init = UnixNanos::default();

        let result = parse_order_status_report(
            order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let order_status_report = result.unwrap();

        assert_eq!(order_status_report.account_id, account_id);
        assert_eq!(order_status_report.instrument_id, instrument_id);
        assert_eq!(
            order_status_report.client_order_id,
            Some(ClientOrderId::new("001BTCUSDT20250106001"))
        );
        assert_eq!(
            order_status_report.venue_order_id,
            VenueOrderId::new("2497956918703120384")
        );
        assert_eq!(order_status_report.order_side, OrderSide::Buy);
        assert_eq!(order_status_report.order_status, OrderStatus::Filled);
        assert_eq!(order_status_report.quantity, Quantity::from("0.03000000"));
        assert_eq!(order_status_report.filled_qty, Quantity::from("0.03000000"));
    }

    #[rstest]
    fn test_parse_fill_report() {
        let json_data = load_test_json("ws_orders.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();
        let order_msg = &data[0];

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let ts_init = UnixNanos::default();

        let result = parse_fill_report(
            order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        );

        assert!(result.is_ok());
        let fill_report = result.unwrap();

        assert_eq!(fill_report.account_id, account_id);
        assert_eq!(fill_report.instrument_id, instrument_id);
        assert_eq!(
            fill_report.client_order_id,
            Some(ClientOrderId::new("001BTCUSDT20250106001"))
        );
        assert_eq!(
            fill_report.venue_order_id,
            VenueOrderId::new("2497956918703120384")
        );
        assert_eq!(fill_report.trade_id, TradeId::from("1518905529"));
        assert_eq!(fill_report.order_side, OrderSide::Buy);
        assert_eq!(fill_report.last_px, Price::from("103698.90"));
        assert_eq!(fill_report.last_qty, Quantity::from("0.03000000"));
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
    }

    #[rstest]
    fn test_parse_book10_msg() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let msgs: Vec<OKXBookMsg> = match event {
            OKXWebSocketEvent::BookData { data, .. } => data,
            _ => panic!("Expected BookData"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let depth10 =
            parse_book10_msg(&msgs[0], instrument_id, 2, 0, UnixNanos::default()).unwrap();

        assert_eq!(depth10.instrument_id, instrument_id);
        assert_eq!(depth10.sequence, 123456);
        assert_eq!(depth10.ts_event, UnixNanos::from(1597026383085000000));
        assert_eq!(depth10.flags, RecordFlag::F_SNAPSHOT as u8);

        // Check bid levels (available in test data: 8 levels)
        assert_eq!(depth10.bids[0].price, Price::from("8476.97"));
        assert_eq!(depth10.bids[0].size, Quantity::from("256"));
        assert_eq!(depth10.bids[0].side, OrderSide::Buy);
        assert_eq!(depth10.bid_counts[0], 12);

        assert_eq!(depth10.bids[1].price, Price::from("8475.55"));
        assert_eq!(depth10.bids[1].size, Quantity::from("101"));
        assert_eq!(depth10.bid_counts[1], 1);

        // Check that levels beyond available data are padded with empty orders
        assert_eq!(depth10.bids[8].price, Price::from("0"));
        assert_eq!(depth10.bids[8].size, Quantity::from("0"));
        assert_eq!(depth10.bid_counts[8], 0);

        // Check ask levels (available in test data: 8 levels)
        assert_eq!(depth10.asks[0].price, Price::from("8476.98"));
        assert_eq!(depth10.asks[0].size, Quantity::from("415"));
        assert_eq!(depth10.asks[0].side, OrderSide::Sell);
        assert_eq!(depth10.ask_counts[0], 13);

        assert_eq!(depth10.asks[1].price, Price::from("8477.00"));
        assert_eq!(depth10.asks[1].size, Quantity::from("7"));
        assert_eq!(depth10.ask_counts[1], 2);

        // Check that levels beyond available data are padded with empty orders
        assert_eq!(depth10.asks[8].price, Price::from("0"));
        assert_eq!(depth10.asks[8].size, Quantity::from("0"));
        assert_eq!(depth10.ask_counts[8], 0);
    }

    #[rstest]
    fn test_parse_book10_msg_vec() {
        let json_data = load_test_json("ws_books_snapshot.json");
        let event: OKXWebSocketEvent = serde_json::from_str(&json_data).unwrap();
        let msgs: Vec<OKXBookMsg> = match event {
            OKXWebSocketEvent::BookData { data, .. } => data,
            _ => panic!("Expected BookData"),
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let depth10_vec =
            parse_book10_msg_vec(msgs, &instrument_id, 2, 0, UnixNanos::default()).unwrap();

        assert_eq!(depth10_vec.len(), 1);

        if let Data::Depth10(d) = &depth10_vec[0] {
            assert_eq!(d.instrument_id, instrument_id);
            assert_eq!(d.sequence, 123456);
            assert_eq!(d.bids[0].price, Price::from("8476.97"));
            assert_eq!(d.asks[0].price, Price::from("8476.98"));
        } else {
            panic!("Expected Depth10");
        }
    }

    #[rstest]
    fn test_parse_fill_report_with_fee_cache() {
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None, // multiplier
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        // First fill: 0.01 BTC out of 0.03 BTC total (1/3)
        let order_msg_1 = OKXOrderMsg {
            acc_fill_sz: Some("0.01".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_1".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-1.0".to_string()), // Total fee so far
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("1234567890"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::PartiallyFilled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.03".to_string(), // Total order size
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_1".to_string(),
            u_time: 1746947317402,
        };

        let fill_report_1 = parse_fill_report(
            &order_msg_1,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        // First fill should get the full fee since there's no previous fee
        assert_eq!(fill_report_1.commission, Money::new(1.0, Currency::USDT()));

        // Second fill: 0.02 BTC more, now 0.03 BTC total (completely filled)
        let order_msg_2 = OKXOrderMsg {
            acc_fill_sz: Some("0.03".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_1".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-3.0".to_string()), // Same total fee
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.02".to_string(),
            fill_time: 1746947317403,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("1234567890"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::Filled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.03".to_string(), // Same total order size
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_2".to_string(),
            u_time: 1746947317403,
        };

        let fill_report_2 = parse_fill_report(
            &order_msg_2,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            Some(fill_report_1.commission),
            Some(fill_report_1.last_qty),
            ts_init,
        )
        .unwrap();

        // Second fill should get total_fee - previous_fee = 3.0 - 1.0 = 2.0
        assert_eq!(fill_report_2.commission, Money::new(2.0, Currency::USDT()));

        // Test passed - fee was correctly split proportionally
    }

    #[rstest]
    fn test_parse_fill_report_with_maker_rebates() {
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        // First fill: maker rebate of $0.5 (OKX sends as "0.5", parse_fee makes it -0.5)
        let order_msg_1 = OKXOrderMsg {
            acc_fill_sz: Some("0.01".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_rebate".to_string(),
            algo_cl_ord_id: None,
            fee: Some("0.5".to_string()), // Rebate: positive value from OKX
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("rebate_order_123"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::PartiallyFilled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_rebate_1".to_string(),
            u_time: 1746947317402,
        };

        let fill_report_1 = parse_fill_report(
            &order_msg_1,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        // First fill gets the full rebate (negative commission)
        assert_eq!(fill_report_1.commission, Money::new(-0.5, Currency::USDT()));

        // Second fill: another maker rebate of $0.3, cumulative now $0.8
        let order_msg_2 = OKXOrderMsg {
            acc_fill_sz: Some("0.02".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_rebate".to_string(),
            algo_cl_ord_id: None,
            fee: Some("0.8".to_string()), // Cumulative rebate
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317403,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("rebate_order_123"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::Filled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_rebate_2".to_string(),
            u_time: 1746947317403,
        };

        let fill_report_2 = parse_fill_report(
            &order_msg_2,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            Some(fill_report_1.commission),
            Some(fill_report_1.last_qty),
            ts_init,
        )
        .unwrap();

        // Second fill: incremental = -0.8 - (-0.5) = -0.3
        assert_eq!(fill_report_2.commission, Money::new(-0.3, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_fill_report_rebate_to_charge_transition() {
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        // First fill: maker rebate of $1.0
        let order_msg_1 = OKXOrderMsg {
            acc_fill_sz: Some("0.01".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_transition".to_string(),
            algo_cl_ord_id: None,
            fee: Some("1.0".to_string()), // Rebate from OKX
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("transition_order_456"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::PartiallyFilled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_transition_1".to_string(),
            u_time: 1746947317402,
        };

        let fill_report_1 = parse_fill_report(
            &order_msg_1,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        // First fill gets rebate (negative)
        assert_eq!(fill_report_1.commission, Money::new(-1.0, Currency::USDT()));

        // Second fill: taker charge of $5.0, net cumulative is now $2.0 charge
        // This is the edge case: incremental = 2.0 - (-1.0) = 3.0, which exceeds total (2.0)
        // But it's legitimate, not corruption
        let order_msg_2 = OKXOrderMsg {
            acc_fill_sz: Some("0.02".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_transition".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-2.0".to_string()), // Now a charge (negative from OKX)
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317403,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("transition_order_456"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::Filled,
            exec_type: crate::common::enums::OKXExecType::Taker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_transition_2".to_string(),
            u_time: 1746947317403,
        };

        let fill_report_2 = parse_fill_report(
            &order_msg_2,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            Some(fill_report_1.commission),
            Some(fill_report_1.last_qty),
            ts_init,
        )
        .unwrap();

        // Second fill: incremental = 2.0 - (-1.0) = 3.0
        // This should NOT trigger corruption detection because previous was negative
        assert_eq!(fill_report_2.commission, Money::new(3.0, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_fill_report_negative_incremental() {
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        // First fill: charge of $2.0
        let order_msg_1 = OKXOrderMsg {
            acc_fill_sz: Some("0.01".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_neg_inc".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-2.0".to_string()),
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("neg_inc_order_789"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::PartiallyFilled,
            exec_type: crate::common::enums::OKXExecType::Taker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_neg_inc_1".to_string(),
            u_time: 1746947317402,
        };

        let fill_report_1 = parse_fill_report(
            &order_msg_1,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(fill_report_1.commission, Money::new(2.0, Currency::USDT()));

        // Second fill: charge reduced to $1.5 total (refund or maker rebate on this fill)
        // Incremental = 1.5 - 2.0 = -0.5 (negative incremental triggers debug log)
        let order_msg_2 = OKXOrderMsg {
            acc_fill_sz: Some("0.02".to_string()),
            avg_px: "50000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::Normal,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "test_order_neg_inc".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-1.5".to_string()), // Total reduced
            fee_ccy: Ustr::from("USDT"),
            fill_px: "50000.0".to_string(),
            fill_sz: "0.01".to_string(),
            fill_time: 1746947317403,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: crate::common::enums::OKXInstrumentType::Swap,
            lever: "2.0".to_string(),
            ord_id: Ustr::from("neg_inc_order_789"),
            ord_type: OKXOrderType::Market,
            pnl: "0".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: crate::common::enums::OKXSide::Buy,
            state: crate::common::enums::OKXOrderStatus::Filled,
            exec_type: crate::common::enums::OKXExecType::Maker,
            sz: "0.02".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "trade_neg_inc_2".to_string(),
            u_time: 1746947317403,
        };

        let fill_report_2 = parse_fill_report(
            &order_msg_2,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            Some(fill_report_1.commission),
            Some(fill_report_1.last_qty),
            ts_init,
        )
        .unwrap();

        // Incremental is negative: 1.5 - 2.0 = -0.5
        assert_eq!(fill_report_2.commission, Money::new(-0.5, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_fill_report_empty_fill_sz_first_fill() {
        let instrument = create_stub_instrument();
        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        let order_msg =
            create_stub_order_msg("", Some("0.01".to_string()), "1234567890", "trade_1");

        let fill_report = parse_fill_report(
            &order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(fill_report.last_qty, Quantity::from("0.01"));
    }

    #[rstest]
    fn test_parse_fill_report_empty_fill_sz_subsequent_fills() {
        let instrument = create_stub_instrument();
        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        let order_msg_1 =
            create_stub_order_msg("", Some("0.01".to_string()), "1234567890", "trade_1");

        let fill_report_1 = parse_fill_report(
            &order_msg_1,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(fill_report_1.last_qty, Quantity::from("0.01"));

        let order_msg_2 =
            create_stub_order_msg("", Some("0.03".to_string()), "1234567890", "trade_2");

        let fill_report_2 = parse_fill_report(
            &order_msg_2,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            Some(fill_report_1.commission),
            Some(fill_report_1.last_qty),
            ts_init,
        )
        .unwrap();

        assert_eq!(fill_report_2.last_qty, Quantity::from("0.02"));
    }

    #[rstest]
    fn test_parse_fill_report_error_both_empty() {
        let instrument = create_stub_instrument();
        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        let order_msg = create_stub_order_msg("", Some("".to_string()), "1234567890", "trade_1");

        let result = parse_fill_report(
            &order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        );

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Cannot determine fill quantity"));
        assert!(err_msg.contains("empty/zero"));
    }

    #[rstest]
    fn test_parse_fill_report_error_acc_fill_sz_none() {
        let instrument = create_stub_instrument();
        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        let order_msg = create_stub_order_msg("", None, "1234567890", "trade_1");

        let result = parse_fill_report(
            &order_msg,
            &InstrumentAny::CryptoPerpetual(instrument),
            account_id,
            None,
            None,
            ts_init,
        );

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Cannot determine fill quantity"));
        assert!(err_msg.contains("acc_fill_sz is None"));
    }

    #[rstest]
    fn test_parse_order_msg_acc_fill_sz_only_update() {
        // Test that we emit fill reports when OKX only updates acc_fill_sz without fill_sz or trade_id
        let instrument = create_stub_instrument();
        let account_id = AccountId::new("OKX-001");
        let ts_init = UnixNanos::default();

        let mut instruments = AHashMap::new();
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let fee_cache = AHashMap::new();
        let mut filled_qty_cache = AHashMap::new();

        // First update: acc_fill_sz = 0.01, no fill_sz, no trade_id
        let msg_1 = create_stub_order_msg("", Some("0.01".to_string()), "1234567890", "");

        let report_1 = parse_order_msg(
            &msg_1,
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            ts_init,
        )
        .unwrap();

        // Should generate a fill report (not a status report)
        assert!(matches!(report_1, ExecutionReport::Fill(_)));
        if let ExecutionReport::Fill(fill) = &report_1 {
            assert_eq!(fill.last_qty, Quantity::from("0.01"));
        }

        // Update cache
        filled_qty_cache.insert(Ustr::from("1234567890"), Quantity::from("0.01"));

        // Second update: acc_fill_sz increased to 0.03, still no fill_sz or trade_id
        let msg_2 = create_stub_order_msg("", Some("0.03".to_string()), "1234567890", "");

        let report_2 = parse_order_msg(
            &msg_2,
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            ts_init,
        )
        .unwrap();

        // Should still generate a fill report for the incremental 0.02
        assert!(matches!(report_2, ExecutionReport::Fill(_)));
        if let ExecutionReport::Fill(fill) = &report_2 {
            assert_eq!(fill.last_qty, Quantity::from("0.02"));
        }
    }

    #[rstest]
    fn test_parse_book10_msg_partial_levels() {
        // Test with fewer than 10 levels - should pad with empty orders
        let book_msg = OKXBookMsg {
            asks: vec![
                OrderBookEntry {
                    price: "8476.98".to_string(),
                    size: "415".to_string(),
                    liquidated_orders_count: "0".to_string(),
                    orders_count: "13".to_string(),
                },
                OrderBookEntry {
                    price: "8477.00".to_string(),
                    size: "7".to_string(),
                    liquidated_orders_count: "0".to_string(),
                    orders_count: "2".to_string(),
                },
            ],
            bids: vec![OrderBookEntry {
                price: "8476.97".to_string(),
                size: "256".to_string(),
                liquidated_orders_count: "0".to_string(),
                orders_count: "12".to_string(),
            }],
            ts: 1597026383085,
            checksum: None,
            prev_seq_id: None,
            seq_id: 123456,
        };

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let depth10 =
            parse_book10_msg(&book_msg, instrument_id, 2, 0, UnixNanos::default()).unwrap();

        // Check that first levels have data
        assert_eq!(depth10.bids[0].price, Price::from("8476.97"));
        assert_eq!(depth10.bids[0].size, Quantity::from("256"));
        assert_eq!(depth10.bid_counts[0], 12);

        // Check that remaining levels are padded with default (empty) orders
        assert_eq!(depth10.bids[1].price, Price::from("0"));
        assert_eq!(depth10.bids[1].size, Quantity::from("0"));
        assert_eq!(depth10.bid_counts[1], 0);

        // Check asks
        assert_eq!(depth10.asks[0].price, Price::from("8476.98"));
        assert_eq!(depth10.asks[1].price, Price::from("8477.00"));
        assert_eq!(depth10.asks[2].price, Price::from("0")); // padded with empty
    }

    #[rstest]
    fn test_parse_algo_order_msg_stop_market() {
        let json_data = load_test_json("ws_orders_algo.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXAlgoOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        // Test first algo order (stop market sell)
        let msg = &data[0];
        assert_eq!(msg.algo_id, "706620792746729472");
        assert_eq!(msg.algo_cl_ord_id, "STOP001BTCUSDT20250120");
        assert_eq!(msg.state, OKXOrderStatus::Live);
        assert_eq!(msg.ord_px, "-1"); // Market order indicator

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create mock instrument
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(), // ts_event
            0.into(), // ts_init
        );
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let result =
            parse_algo_order_msg(msg.clone(), account_id, &instruments, UnixNanos::default());

        assert!(result.is_ok());
        let report = result.unwrap();

        if let ExecutionReport::Order(status_report) = report {
            assert_eq!(status_report.order_type, OrderType::StopMarket);
            assert_eq!(status_report.order_side, OrderSide::Sell);
            assert_eq!(status_report.quantity, Quantity::from("0.01000000"));
            assert_eq!(status_report.trigger_price, Some(Price::from("95000.00")));
            assert_eq!(status_report.trigger_type, Some(TriggerType::LastPrice));
            assert_eq!(status_report.price, None); // No limit price for market orders
        } else {
            panic!("Expected Order report");
        }
    }

    #[rstest]
    fn test_parse_algo_order_msg_stop_limit() {
        let json_data = load_test_json("ws_orders_algo.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXAlgoOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        // Test second algo order (stop limit buy)
        let msg = &data[1];
        assert_eq!(msg.algo_id, "706620792746729473");
        assert_eq!(msg.state, OKXOrderStatus::Live);
        assert_eq!(msg.ord_px, "106000"); // Limit price

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create mock instrument
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(), // ts_event
            0.into(), // ts_init
        );
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let result =
            parse_algo_order_msg(msg.clone(), account_id, &instruments, UnixNanos::default());

        assert!(result.is_ok());
        let report = result.unwrap();

        if let ExecutionReport::Order(status_report) = report {
            assert_eq!(status_report.order_type, OrderType::StopLimit);
            assert_eq!(status_report.order_side, OrderSide::Buy);
            assert_eq!(status_report.quantity, Quantity::from("0.02000000"));
            assert_eq!(status_report.trigger_price, Some(Price::from("105000.00")));
            assert_eq!(status_report.trigger_type, Some(TriggerType::MarkPrice));
            assert_eq!(status_report.price, Some(Price::from("106000.00"))); // Has limit price
        } else {
            panic!("Expected Order report");
        }
    }

    #[rstest]
    fn test_parse_trigger_order_from_regular_channel() {
        let json_data = load_test_json("ws_orders_trigger.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        // Test triggered order that came through regular orders channel
        let msg = &data[0];
        assert_eq!(msg.ord_type, OKXOrderType::Trigger);
        assert_eq!(msg.state, OKXOrderStatus::Filled);

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create mock instrument
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(), // ts_event
            0.into(), // ts_init
        );
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );
        let fee_cache = AHashMap::new();
        let filled_qty_cache = AHashMap::new();

        let result = parse_order_msg_vec(
            vec![msg.clone()],
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let reports = result.unwrap();
        assert_eq!(reports.len(), 1);

        if let ExecutionReport::Fill(fill_report) = &reports[0] {
            assert_eq!(fill_report.order_side, OrderSide::Sell);
            assert_eq!(fill_report.last_qty, Quantity::from("0.01000000"));
            assert_eq!(fill_report.last_px, Price::from("101950.00"));
        } else {
            panic!("Expected Fill report for filled trigger order");
        }
    }

    #[rstest]
    fn test_parse_liquidation_order() {
        let json_data = load_test_json("ws_orders_liquidation.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        // Test liquidation order
        let msg = &data[0];
        assert_eq!(msg.category, OKXOrderCategory::FullLiquidation);
        assert_eq!(msg.state, OKXOrderStatus::Filled);
        assert_eq!(msg.inst_id.as_str(), "BTC-USDT-SWAP");

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create mock instrument
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(), // ts_event
            0.into(), // ts_init
        );
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );
        let fee_cache = AHashMap::new();
        let filled_qty_cache = AHashMap::new();

        let result = parse_order_msg_vec(
            vec![msg.clone()],
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let reports = result.unwrap();
        assert_eq!(reports.len(), 1);

        // Verify it's a fill report for a liquidation
        if let ExecutionReport::Fill(fill_report) = &reports[0] {
            assert_eq!(fill_report.order_side, OrderSide::Sell);
            assert_eq!(fill_report.last_qty, Quantity::from("0.50000000"));
            assert_eq!(fill_report.last_px, Price::from("40000.00"));
            assert_eq!(fill_report.liquidity_side, LiquiditySide::Taker);
        } else {
            panic!("Expected Fill report for liquidation order");
        }
    }

    #[rstest]
    fn test_parse_adl_order() {
        let json_data = load_test_json("ws_orders_adl.json");
        let ws_msg: serde_json::Value = serde_json::from_str(&json_data).unwrap();
        let data: Vec<OKXOrderMsg> = serde_json::from_value(ws_msg["data"].clone()).unwrap();

        // Test ADL order
        let msg = &data[0];
        assert_eq!(msg.category, OKXOrderCategory::Adl);
        assert_eq!(msg.state, OKXOrderStatus::Filled);
        assert_eq!(msg.inst_id.as_str(), "ETH-USDT-SWAP");

        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        // Create mock instrument
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("ETH-USDT-SWAP"),
            Currency::ETH(),
            Currency::USDT(),
            Currency::USDT(),
            false, // is_inverse
            2,     // price_precision
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(), // ts_event
            0.into(), // ts_init
        );
        instruments.insert(
            Ustr::from("ETH-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );
        let fee_cache = AHashMap::new();
        let filled_qty_cache = AHashMap::new();

        let result = parse_order_msg_vec(
            vec![msg.clone()],
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let reports = result.unwrap();
        assert_eq!(reports.len(), 1);

        // Verify it's a fill report for ADL
        if let ExecutionReport::Fill(fill_report) = &reports[0] {
            assert_eq!(fill_report.order_side, OrderSide::Buy);
            assert_eq!(fill_report.last_qty, Quantity::from("0.30000000"));
            assert_eq!(fill_report.last_px, Price::from("41000.00"));
            assert_eq!(fill_report.liquidity_side, LiquiditySide::Taker);
        } else {
            panic!("Expected Fill report for ADL order");
        }
    }

    #[rstest]
    fn test_parse_unknown_category_graceful_fallback() {
        // Test that unknown/future category values deserialize as Other instead of failing
        let json_with_unknown_category = r#"{
            "category": "some_future_category_we_dont_know"
        }"#;

        let result: Result<serde_json::Value, _> = serde_json::from_str(json_with_unknown_category);
        assert!(result.is_ok());

        // Test deserialization of the category field directly
        let category_result: Result<OKXOrderCategory, _> =
            serde_json::from_str(r#""some_future_category""#);
        assert!(category_result.is_ok());
        assert_eq!(category_result.unwrap(), OKXOrderCategory::Other);

        // Verify known categories still work
        let normal: OKXOrderCategory = serde_json::from_str(r#""normal""#).unwrap();
        assert_eq!(normal, OKXOrderCategory::Normal);

        let twap: OKXOrderCategory = serde_json::from_str(r#""twap""#).unwrap();
        assert_eq!(twap, OKXOrderCategory::Twap);
    }

    #[rstest]
    fn test_parse_partial_liquidation_order() {
        // Create a test message with partial liquidation category
        let account_id = AccountId::new("OKX-001");
        let mut instruments = AHashMap::new();

        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USDT-SWAP"),
            Currency::BTC(),
            Currency::USDT(),
            Currency::USDT(),
            false,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(),
            0.into(),
        );
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let partial_liq_msg = OKXOrderMsg {
            acc_fill_sz: Some("0.25".to_string()),
            avg_px: "39000.0".to_string(),
            c_time: 1746947317401,
            cancel_source: None,
            cancel_source_reason: None,
            category: OKXOrderCategory::PartialLiquidation,
            ccy: Ustr::from("USDT"),
            cl_ord_id: "".to_string(),
            algo_cl_ord_id: None,
            fee: Some("-9.75".to_string()),
            fee_ccy: Ustr::from("USDT"),
            fill_px: "39000.0".to_string(),
            fill_sz: "0.25".to_string(),
            fill_time: 1746947317402,
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            lever: "10.0".to_string(),
            ord_id: Ustr::from("2497956918703120888"),
            ord_type: OKXOrderType::Market,
            pnl: "-2500".to_string(),
            pos_side: OKXPositionSide::Long,
            px: "".to_string(),
            reduce_only: "false".to_string(),
            side: OKXSide::Sell,
            state: OKXOrderStatus::Filled,
            exec_type: OKXExecType::Taker,
            sz: "0.25".to_string(),
            td_mode: OKXTradeMode::Isolated,
            tgt_ccy: None,
            trade_id: "1518905888".to_string(),
            u_time: 1746947317402,
        };

        let fee_cache = AHashMap::new();
        let filled_qty_cache = AHashMap::new();
        let result = parse_order_msg(
            &partial_liq_msg,
            account_id,
            &instruments,
            &fee_cache,
            &filled_qty_cache,
            UnixNanos::default(),
        );

        assert!(result.is_ok());
        let report = result.unwrap();

        // Verify it's a fill report for partial liquidation
        if let ExecutionReport::Fill(fill_report) = report {
            assert_eq!(fill_report.order_side, OrderSide::Sell);
            assert_eq!(fill_report.last_qty, Quantity::from("0.25000000"));
            assert_eq!(fill_report.last_px, Price::from("39000.00"));
        } else {
            panic!("Expected Fill report for partial liquidation order");
        }
    }

    #[rstest]
    fn test_websocket_instrument_update_preserves_cached_fees() {
        use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};

        use crate::common::{models::OKXInstrument, parse::parse_instrument_any};

        let ts_init = UnixNanos::default();

        // Create initial instrument with fees (simulating HTTP load)
        let initial_fees = (
            Some(Decimal::new(8, 4)),  // maker_fee = 0.0008
            Some(Decimal::new(10, 4)), // taker_fee = 0.0010
        );

        // Deserialize initial instrument from JSON
        let initial_inst_json = serde_json::json!({
            "instType": "SPOT",
            "instId": "BTC-USD",
            "baseCcy": "BTC",
            "quoteCcy": "USD",
            "settleCcy": "",
            "ctVal": "",
            "ctMult": "",
            "ctValCcy": "",
            "optType": "",
            "stk": "",
            "listTime": "1733454000000",
            "expTime": "",
            "lever": "",
            "tickSz": "0.1",
            "lotSz": "0.00000001",
            "minSz": "0.00001",
            "ctType": "linear",
            "alias": "",
            "state": "live",
            "maxLmtSz": "9999999999",
            "maxMktSz": "1000000",
            "maxTwapSz": "9999999999.0000000000000000",
            "maxIcebergSz": "9999999999.0000000000000000",
            "maxTriggerSz": "9999999999.0000000000000000",
            "maxStopSz": "1000000",
            "uly": "",
            "instFamily": "",
            "ruleType": "normal",
            "maxLmtAmt": "20000000",
            "maxMktAmt": "1000000"
        });

        let initial_inst: OKXInstrument = serde_json::from_value(initial_inst_json)
            .expect("Failed to deserialize initial instrument");

        // Parse initial instrument with fees
        let parsed_initial = parse_instrument_any(
            &initial_inst,
            None,
            None,
            initial_fees.0,
            initial_fees.1,
            ts_init,
        )
        .expect("Failed to parse initial instrument")
        .expect("Initial instrument should not be None");

        // Verify fees were applied
        if let InstrumentAny::CurrencyPair(ref pair) = parsed_initial {
            assert_eq!(pair.maker_fee, Decimal::new(8, 4));
            assert_eq!(pair.taker_fee, Decimal::new(10, 4));
        } else {
            panic!("Expected CurrencyPair instrument");
        }

        // Build instrument cache with the initial instrument
        let mut instruments_cache = AHashMap::new();
        instruments_cache.insert(Ustr::from("BTC-USD"), parsed_initial);

        // Create WebSocket update message (same structure as initial, simulating a WebSocket update)
        let ws_update = serde_json::json!({
            "instType": "SPOT",
            "instId": "BTC-USD",
            "baseCcy": "BTC",
            "quoteCcy": "USD",
            "settleCcy": "",
            "ctVal": "",
            "ctMult": "",
            "ctValCcy": "",
            "optType": "",
            "stk": "",
            "listTime": "1733454000000",
            "expTime": "",
            "lever": "",
            "tickSz": "0.1",
            "lotSz": "0.00000001",
            "minSz": "0.00001",
            "ctType": "linear",
            "alias": "",
            "state": "live",
            "maxLmtSz": "9999999999",
            "maxMktSz": "1000000",
            "maxTwapSz": "9999999999.0000000000000000",
            "maxIcebergSz": "9999999999.0000000000000000",
            "maxTriggerSz": "9999999999.0000000000000000",
            "maxStopSz": "1000000",
            "uly": "",
            "instFamily": "",
            "ruleType": "normal",
            "maxLmtAmt": "20000000",
            "maxMktAmt": "1000000"
        });

        let instrument_id = InstrumentId::from("BTC-USD.OKX");
        let mut funding_cache = AHashMap::new();

        // Parse WebSocket update with cache
        let result = parse_ws_message_data(
            &OKXWsChannel::Instruments,
            ws_update,
            &instrument_id,
            2,
            8,
            ts_init,
            &mut funding_cache,
            &instruments_cache,
        )
        .expect("Failed to parse WebSocket instrument update");

        // Verify the update preserves the cached fees
        if let Some(NautilusWsMessage::Instrument(boxed_inst)) = result {
            if let InstrumentAny::CurrencyPair(pair) = *boxed_inst {
                assert_eq!(
                    pair.maker_fee,
                    Decimal::new(8, 4),
                    "Maker fee should be preserved from cache"
                );
                assert_eq!(
                    pair.taker_fee,
                    Decimal::new(10, 4),
                    "Taker fee should be preserved from cache"
                );
            } else {
                panic!("Expected CurrencyPair instrument from WebSocket update");
            }
        } else {
            panic!("Expected Instrument message from WebSocket update");
        }
    }
}
