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

use std::{num::NonZero, str::FromStr};

use ahash::AHashMap;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime, uuid::UUID4};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
        depth::DEPTH10_LEN,
    },
    enums::{
        AccountType, AggregationSource, BarAggregation, OrderSide, OrderStatus, PriceType,
        RecordFlag,
    },
    events::account::state::AccountState,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, Symbol, TradeId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;
use uuid::Uuid;

use super::{
    enums::{BitmexAction, BitmexWsTopic},
    messages::{
        BitmexExecutionMsg, BitmexFundingMsg, BitmexInstrumentMsg, BitmexMarginMsg,
        BitmexOrderBook10Msg, BitmexOrderBookMsg, BitmexOrderMsg, BitmexPositionMsg,
        BitmexQuoteMsg, BitmexTradeBinMsg, BitmexTradeMsg, BitmexWalletMsg,
    },
};
use crate::common::{
    consts::BITMEX_VENUE,
    enums::{BitmexExecInstruction, BitmexExecType, BitmexSide},
    parse::{
        map_bitmex_currency, parse_contracts_quantity, parse_frac_quantity, parse_instrument_id,
        parse_liquidity_side, parse_optional_datetime_to_unix_nanos, parse_order_status,
        parse_order_type, parse_position_side, parse_time_in_force,
    },
};

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

/// Check if a symbol is an index symbol (starts with '.').
///
/// Index symbols in BitMEX represent indices like `.BXBT` and have different
/// behavior from regular instruments:
/// - They only have a single price value (no bid/ask spread).
/// - They don't have trades or quotes.
/// - Their price is delivered via the `lastPrice` field.
#[inline]
#[must_use]
pub fn is_index_symbol(symbol: &Ustr) -> bool {
    symbol.starts_with('.')
}

#[must_use]
pub fn parse_book_msg_vec(
    data: Vec<BitmexOrderBookMsg>,
    action: BitmexAction,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut deltas = Vec::with_capacity(data.len());

    for msg in data {
        deltas.push(Data::Delta(parse_book_msg(
            &msg,
            &action,
            price_precision,
            ts_init,
        )));
    }
    deltas
}

#[must_use]
pub fn parse_book10_msg_vec(
    data: Vec<BitmexOrderBook10Msg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut depths = Vec::with_capacity(data.len());

    for msg in data {
        depths.push(Data::Depth10(Box::new(parse_book10_msg(
            &msg,
            price_precision,
            ts_init,
        ))));
    }
    depths
}

#[must_use]
pub fn parse_trade_msg_vec(
    data: Vec<BitmexTradeMsg>,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());

    for msg in data {
        trades.push(Data::Trade(parse_trade_msg(&msg, price_precision, ts_init)));
    }
    trades
}

#[must_use]
pub fn parse_trade_bin_msg_vec(
    data: Vec<BitmexTradeBinMsg>,
    topic: BitmexWsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());

    for msg in data {
        trades.push(Data::Bar(parse_trade_bin_msg(
            &msg,
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
    msg: &BitmexOrderBookMsg,
    action: &BitmexAction,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let flags = if action == &BitmexAction::Insert {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let instrument_id = parse_instrument_id(msg.symbol);
    let action = action.as_book_action();
    let price = Price::new(msg.price, price_precision);
    let side = msg.side.as_order_side();
    let size = parse_contracts_quantity(msg.size.unwrap_or(0));
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

/// Parses an `OrderBook10` message into an `OrderBookDepth10` object.
///
/// # Panics
///
/// Panics if the bid or ask arrays cannot be converted to exactly 10 elements.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn parse_book10_msg(
    msg: &BitmexOrderBook10Msg,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDepth10 {
    let instrument_id = parse_instrument_id(msg.symbol);

    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);

    // Initialized with zeros
    let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
    let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];

    for (i, level) in msg.bids.iter().enumerate() {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::new(level[0], price_precision),
            parse_frac_quantity(level[1], 0),
            0,
        );

        bids.push(bid_order);
        bid_counts[i] = 1;
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::new(level[0], price_precision),
            parse_frac_quantity(level[1], 0),
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
    msg: &BitmexQuoteMsg,
    last_quote: &QuoteTick,
    price_precision: u8,
    ts_init: UnixNanos,
) -> QuoteTick {
    let instrument_id = parse_instrument_id(msg.symbol);

    let bid_price = match msg.bid_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.bid_price,
    };

    let ask_price = match msg.ask_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.ask_price,
    };

    let bid_size = match msg.bid_size {
        Some(size) => parse_contracts_quantity(size),
        None => last_quote.bid_size,
    };

    let ask_size = match msg.ask_size {
        Some(size) => parse_contracts_quantity(size),
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
pub fn parse_trade_msg(msg: &BitmexTradeMsg, price_precision: u8, ts_init: UnixNanos) -> TradeTick {
    let instrument_id = parse_instrument_id(msg.symbol);
    let price = Price::new(msg.price, price_precision);
    let size = parse_contracts_quantity(msg.size);
    let aggressor_side = msg.side.as_aggressor_side();
    let trade_id = TradeId::new(
        msg.trd_match_id
            .map_or_else(|| Uuid::new_v4().to_string(), |uuid| uuid.to_string()),
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
    msg: &BitmexTradeBinMsg,
    topic: &BitmexWsTopic,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Bar {
    let instrument_id = parse_instrument_id(msg.symbol);
    let spec = bar_spec_from_topic(topic);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);
    let volume = parse_contracts_quantity(msg.volume as u64);
    let ts_event = UnixNanos::from(msg.timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Converts a WebSocket topic to a bar specification.
///
/// # Panics
///
/// Panics if the topic is not a valid bar topic (`TradeBin1m`, `TradeBin5m`, `TradeBin1h`, or `TradeBin1d`).
#[must_use]
pub fn bar_spec_from_topic(topic: &BitmexWsTopic) -> BarSpecification {
    match topic {
        BitmexWsTopic::TradeBin1m => BAR_SPEC_1_MINUTE,
        BitmexWsTopic::TradeBin5m => BAR_SPEC_5_MINUTE,
        BitmexWsTopic::TradeBin1h => BAR_SPEC_1_HOUR,
        BitmexWsTopic::TradeBin1d => BAR_SPEC_1_DAY,
        _ => panic!("Bar specification not supported for {topic}"),
    }
}

/// Converts a bar specification to a WebSocket topic.
///
/// # Panics
///
/// Panics if the specification is not one of the supported values (1m, 5m, 1h, or 1d).
#[must_use]
pub fn topic_from_bar_spec(spec: BarSpecification) -> BitmexWsTopic {
    match spec {
        BAR_SPEC_1_MINUTE => BitmexWsTopic::TradeBin1m,
        BAR_SPEC_5_MINUTE => BitmexWsTopic::TradeBin5m,
        BAR_SPEC_1_HOUR => BitmexWsTopic::TradeBin1h,
        BAR_SPEC_1_DAY => BitmexWsTopic::TradeBin1d,
        _ => panic!("Bar specification not supported {spec}"),
    }
}

/// Parse a BitMEX WebSocket order message into a Nautilus `OrderStatusReport`.
///
/// # Panics
///
/// Panics if required fields are missing or invalid.
///
/// # References
///
/// <https://www.bitmex.com/app/wsAPI#Order>
///
#[must_use]
pub fn parse_order_msg(msg: &BitmexOrderMsg, price_precision: u8) -> OrderStatusReport {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account)); // TODO: Revisit
    let instrument_id = parse_instrument_id(msg.symbol);
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let order_side: OrderSide = BitmexSide::from(msg.side).into();
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

    if let Some(cl_ord_id) = &msg.cl_ord_id {
        report = report.with_client_order_id(ClientOrderId::new(cl_ord_id));
    }

    if let Some(cl_ord_link_id) = &msg.cl_ord_link_id {
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

    if let Some(exec_inst) = &msg.exec_inst {
        match exec_inst {
            BitmexExecInstruction::ParticipateDoNotInitiate => {
                report = report.with_post_only(true);
            }
            BitmexExecInstruction::ReduceOnly => {
                report = report.with_reduce_only(true);
            }
            _ => {}
        }
    }

    // Check if this is a canceled post-only order (BitMEX cancels instead of rejecting)
    // We need to preserve the rejection reason for the execution client to handle
    if order_status == OrderStatus::Canceled
        && let Some(reason_str) = msg.ord_rej_reason.or(msg.text)
    {
        report = report.with_cancel_reason(reason_str.to_string());
    }

    report
}

/// Parse a BitMEX WebSocket execution message into a Nautilus `FillReport`.
///
/// # Panics
///
/// Panics if required fields are missing or invalid.
///
/// # References
///
/// <https://www.bitmex.com/app/wsAPI#Execution>
///
pub fn parse_execution_msg(msg: BitmexExecutionMsg, price_precision: u8) -> Option<FillReport> {
    if msg.exec_type != Some(BitmexExecType::Trade) {
        return None;
    }

    let account_id = AccountId::new(format!("BITMEX-{}", msg.account?));
    let instrument_id = parse_instrument_id(msg.symbol?);
    let venue_order_id = VenueOrderId::new(msg.order_id?.to_string());
    let trade_id = TradeId::new(msg.trd_match_id?.to_string());
    let order_side: OrderSide = msg
        .side
        .map(BitmexSide::from)
        .map_or(OrderSide::NoOrderSide, std::convert::Into::into);
    let last_qty = Quantity::from(msg.last_qty?);
    let last_px = Price::new(msg.last_px?, price_precision);
    let settlement_currency_str = msg.settl_currency.unwrap_or(Ustr::from("XBT"));
    let mapped_currency = map_bitmex_currency(settlement_currency_str.as_str());
    let commission = Money::new(
        msg.commission.unwrap_or(0.0),
        Currency::from(mapped_currency.as_str()),
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
///
/// <https://www.bitmex.com/app/wsAPI#Position>
#[must_use]
pub fn parse_position_msg(msg: BitmexPositionMsg) -> PositionStatusReport {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let instrument_id = parse_instrument_id(msg.symbol);
    let position_side = parse_position_side(msg.current_qty).as_specified();
    let quantity = Quantity::from(msg.current_qty.map_or(0, i64::abs));
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
/// Returns a Vec of Data containing mark and/or index price updates
/// or an empty Vec if no relevant price is present.
#[must_use]
pub fn parse_instrument_msg(
    msg: BitmexInstrumentMsg,
    instruments_cache: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
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

    let instrument_id = InstrumentId::new(Symbol::from_ustr_unchecked(msg.symbol), *BITMEX_VENUE);
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "");

    // Look up instrument for proper precision
    let price_precision = match instruments_cache.get(&Ustr::from(&msg.symbol)) {
        Some(instrument) => instrument.price_precision(),
        None => {
            // BitMEX sends updates for all instruments on the instrument channel,
            // but we only cache instruments that are explicitly requested.
            // Index instruments (starting with '.') are not loaded via regular API endpoints.
            if is_index {
                tracing::trace!(
                    "Index instrument {} not in cache, skipping price updates",
                    msg.symbol
                );
            } else {
                tracing::debug!(
                    "Instrument {} not in cache, skipping price updates",
                    msg.symbol
                );
            }
            return updates;
        }
    };

    // Add mark price update if present
    // For index symbols, markPrice equals lastPrice and is valid to emit
    if let Some(mark_price) = msg.mark_price {
        let price = Price::new(mark_price, price_precision);
        updates.push(Data::MarkPriceUpdate(MarkPriceUpdate::new(
            instrument_id,
            price,
            ts_event,
            ts_init,
        )));
    }

    // Add index price update if present
    if let Some(index_price) = effective_index_price {
        let price = Price::new(index_price, price_precision);
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
/// Note: This returns `FundingRateUpdate` directly, not wrapped in Data enum,
/// to keep it separate from the FFI layer.
pub fn parse_funding_msg(msg: BitmexFundingMsg, ts_init: UnixNanos) -> Option<FundingRateUpdate> {
    let instrument_id = InstrumentId::from(format!("{}.BITMEX", msg.symbol).as_str());
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "");

    // Convert funding rate to Decimal
    let rate = match Decimal::from_str(&msg.funding_rate.to_string()) {
        Ok(rate) => rate,
        Err(e) => {
            tracing::error!("Failed to parse funding rate: {e}");
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

/// Parse a BitMEX wallet message into an AccountState.
///
/// BitMEX uses XBT (satoshis) as the base unit for Bitcoin.
/// 1 XBT = 0.00000001 BTC (1 satoshi).
///
/// # Panics
///
/// Panics if the balance calculation is invalid (total != locked + free).
#[must_use]
pub fn parse_wallet_msg(msg: BitmexWalletMsg, ts_init: UnixNanos) -> AccountState {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));

    // Map BitMEX currency to standard currency code
    let currency_str = crate::common::parse::map_bitmex_currency(msg.currency.as_str());
    let currency = Currency::from(currency_str.as_str());

    // BitMEX returns values in satoshis for BTC (XBt) or microunits for USDT/LAMp
    let divisor = if msg.currency == "XBt" {
        100_000_000.0 // Satoshis to BTC
    } else if msg.currency == "USDt" || msg.currency == "LAMp" {
        1_000_000.0 // Microunits to units
    } else {
        1.0
    };
    let amount = msg.amount.unwrap_or(0) as f64 / divisor;

    let total = Money::new(amount, currency);
    let locked = Money::new(0.0, currency); // No locked amount info available
    let free = total - locked;

    let balance = AccountBalance::new_checked(total, locked, free)
        .expect("Balance calculation should be valid");

    AccountState::new(
        account_id,
        AccountType::Margin,
        vec![balance],
        vec![], // margins will be added separately
        true,   // is_reported
        UUID4::new(),
        ts_init,
        ts_init,
        None,
    )
}

/// Parse a BitMEX margin message into margin balance information.
///
/// This creates a MarginBalance that can be added to an AccountState.
#[must_use]
pub fn parse_margin_msg(msg: BitmexMarginMsg, instrument_id: InstrumentId) -> MarginBalance {
    // Map BitMEX currency to standard currency code
    let currency_str = crate::common::parse::map_bitmex_currency(msg.currency.as_str());
    let currency = Currency::from(currency_str.as_str());

    // BitMEX returns values in satoshis for BTC (XBt) or microunits for USDT/LAMp
    let divisor = if msg.currency == "XBt" {
        100_000_000.0 // Satoshis to BTC
    } else if msg.currency == "USDt" || msg.currency == "LAMp" {
        1_000_000.0 // Microunits to units
    } else {
        1.0
    };

    let initial = (msg.init_margin.unwrap_or(0) as f64 / divisor).max(0.0);
    let maintenance = (msg.maint_margin.unwrap_or(0) as f64 / divisor).max(0.0);
    let _unrealized = msg.unrealised_pnl.unwrap_or(0) as f64 / divisor;

    MarginBalance::new(
        Money::new(initial, currency),
        Money::new(maintenance, currency),
        instrument_id,
    )
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::quote::QuoteTick,
        enums::{
            AggressorSide, BookAction, LiquiditySide, OrderStatus, OrderType, PositionSide,
            TimeInForce,
        },
        identifiers::{InstrumentId, Symbol},
        instruments::{CryptoPerpetual, any::InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::{
        enums::{BitmexExecType, BitmexOrderStatus},
        testing::load_test_json,
    };

    // Helper function to create a test perpetual instrument for tests
    fn create_test_perpetual_instrument() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::from("XBTUSD.BITMEX"),
            Symbol::new("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::BTC(),
            true, // is_inverse
            1,    // price_precision
            0,    // size_precision
            Price::from("0.5"),
            Quantity::from(1),
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
        ))
    }

    #[rstest]
    fn test_orderbook_l2_message() {
        let json_data = load_test_json("ws_orderbook_l2.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: BitmexOrderBookMsg = serde_json::from_str(&json_data).unwrap();

        // Test Insert action
        let delta = parse_book_msg(&msg, &BitmexAction::Insert, 1, UnixNanos::from(3));
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
        let delta = parse_book_msg(&msg, &BitmexAction::Update, 1, UnixNanos::from(3));
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.action, BookAction::Update);
    }

    #[rstest]
    fn test_orderbook10_message() {
        let json_data = load_test_json("ws_orderbook_10.json");
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: BitmexOrderBook10Msg = serde_json::from_str(&json_data).unwrap();
        let depth10 = parse_book10_msg(&msg, 1, UnixNanos::from(3));

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
        let msg: BitmexQuoteMsg = serde_json::from_str(&json_data).unwrap();
        let quote = parse_quote_msg(&msg, &last_quote, 2, UnixNanos::from(3));

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
        let msg: BitmexTradeMsg = serde_json::from_str(&json_data).unwrap();
        let trade = parse_trade_msg(&msg, 1, UnixNanos::from(3));

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
        let topic = BitmexWsTopic::TradeBin1m;

        let msg: BitmexTradeBinMsg = serde_json::from_str(&json_data).unwrap();
        let bar = parse_trade_bin_msg(&msg, &topic, 1, UnixNanos::from(3));

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
        let msg: BitmexOrderMsg = serde_json::from_str(&json_data).unwrap();
        let report = parse_order_msg(&msg, 1);

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
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from(100));
        assert_eq!(report.filled_qty, Quantity::from(0));
        assert_eq!(report.price.unwrap(), Price::from("98000.0"));
        assert_eq!(report.ts_accepted, 1732530600000000000); // 2024-11-25T10:30:00.000Z
    }

    #[rstest]
    fn test_parse_execution_msg() {
        let json_data = load_test_json("ws_execution.json");
        let msg: BitmexExecutionMsg = serde_json::from_str(&json_data).unwrap();
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
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Settlement);

        let result = parse_execution_msg(msg, 1);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_cancel_reject_execution() {
        // Test that CancelReject messages can be parsed (even without symbol)
        let json = r#"{
            "execID":"00000000-006d-1000-0000-001e7f5081ad",
            "orderID":"ece0a2cc-7729-4f4c-bc6c-65d7c723e75b",
            "account":1667725,
            "execType":"CancelReject",
            "ordStatus":"Rejected",
            "workingIndicator":false,
            "ordRejReason":"Invalid orderID",
            "text":"Invalid orderID",
            "transactTime":"2025-09-05T05:38:28.001Z",
            "timestamp":"2025-09-05T05:38:28.001Z"
        }"#;

        let msg: BitmexExecutionMsg = serde_json::from_str(json).unwrap();
        assert_eq!(msg.exec_type, Some(BitmexExecType::CancelReject));
        assert_eq!(msg.ord_status, Some(BitmexOrderStatus::Rejected));
        assert_eq!(msg.symbol, None);

        // Should return None since it's not a Trade
        let result = parse_execution_msg(msg, 1);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_position_msg() {
        let json_data = load_test_json("ws_position.json");
        let msg: BitmexPositionMsg = serde_json::from_str(&json_data).unwrap();
        let report = parse_position_msg(msg);

        assert_eq!(report.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(report.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(report.position_side.as_position_side(), PositionSide::Long);
        assert_eq!(report.quantity, Quantity::from(1000));
        assert!(report.venue_position_id.is_none());
        assert_eq!(report.ts_last, 1732530900789000000); // 2024-11-25T10:35:00.789Z
    }

    #[rstest]
    fn test_parse_position_msg_short() {
        let mut msg: BitmexPositionMsg =
            serde_json::from_str(&load_test_json("ws_position.json")).unwrap();
        msg.current_qty = Some(-500);

        let report = parse_position_msg(msg);
        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity, Quantity::from(500));
    }

    #[rstest]
    fn test_parse_position_msg_flat() {
        let mut msg: BitmexPositionMsg =
            serde_json::from_str(&load_test_json("ws_position.json")).unwrap();
        msg.current_qty = Some(0);

        let report = parse_position_msg(msg);
        assert_eq!(report.position_side.as_position_side(), PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::from(0));
    }

    #[rstest]
    fn test_parse_wallet_msg() {
        let json_data = load_test_json("ws_wallet.json");
        let msg: BitmexWalletMsg = serde_json::from_str(&json_data).unwrap();
        let ts_init = UnixNanos::from(1);
        let account_state = parse_wallet_msg(msg, ts_init);

        assert_eq!(account_state.account_id.to_string(), "BITMEX-1234567");
        assert!(!account_state.balances.is_empty());
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency.code.to_string(), "XBT");
        // Amount should be converted from satoshis (100005180 / 100_000_000.0 = 1.0000518)
        assert!((balance.total.as_f64() - 1.0000518).abs() < 1e-7);
    }

    #[rstest]
    fn test_parse_wallet_msg_no_amount() {
        let mut msg: BitmexWalletMsg =
            serde_json::from_str(&load_test_json("ws_wallet.json")).unwrap();
        msg.amount = None;

        let ts_init = UnixNanos::from(1);
        let account_state = parse_wallet_msg(msg, ts_init);
        let balance = &account_state.balances[0];
        assert_eq!(balance.total.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_margin_msg() {
        let json_data = load_test_json("ws_margin.json");
        let msg: BitmexMarginMsg = serde_json::from_str(&json_data).unwrap();
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let margin_balance = parse_margin_msg(msg, instrument_id);

        assert_eq!(margin_balance.currency.code.to_string(), "XBT");
        assert_eq!(margin_balance.instrument_id, instrument_id);
        // Values should be converted from satoshis to BTC
        // initMargin is 0 in test data, so should be 0.0
        assert_eq!(margin_balance.initial.as_f64(), 0.0);
        // maintMargin is 15949 satoshis = 0.00015949 BTC
        assert!((margin_balance.maintenance.as_f64() - 0.00015949).abs() < 1e-8);
    }

    #[rstest]
    fn test_parse_margin_msg_no_available() {
        let mut msg: BitmexMarginMsg =
            serde_json::from_str(&load_test_json("ws_margin.json")).unwrap();
        msg.available_margin = None;

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let margin_balance = parse_margin_msg(msg, instrument_id);
        // Should still have valid margin values even if available_margin is None
        assert!(margin_balance.initial.as_f64() >= 0.0);
        assert!(margin_balance.maintenance.as_f64() >= 0.0);
    }

    #[rstest]
    fn test_parse_instrument_msg_both_prices() {
        let json_data = load_test_json("ws_instrument.json");
        let msg: BitmexInstrumentMsg = serde_json::from_str(&json_data).unwrap();

        // Create cache with test instrument
        let mut instruments_cache = AHashMap::new();
        let test_instrument = create_test_perpetual_instrument();
        instruments_cache.insert(Ustr::from("XBTUSD"), test_instrument);

        let updates = parse_instrument_msg(msg, &instruments_cache, UnixNanos::from(1));

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
        let mut msg: BitmexInstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.index_price = None;

        // Create cache with test instrument
        let mut instruments_cache = AHashMap::new();
        let test_instrument = create_test_perpetual_instrument();
        instruments_cache.insert(Ustr::from("XBTUSD"), test_instrument);

        let updates = parse_instrument_msg(msg, &instruments_cache, UnixNanos::from(1));

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
        let mut msg: BitmexInstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.mark_price = None;

        // Create cache with test instrument
        let mut instruments_cache = AHashMap::new();
        let test_instrument = create_test_perpetual_instrument();
        instruments_cache.insert(Ustr::from("XBTUSD"), test_instrument);

        let updates = parse_instrument_msg(msg, &instruments_cache, UnixNanos::from(1));

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
        let mut msg: BitmexInstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.mark_price = None;
        msg.index_price = None;
        msg.last_price = None;

        // Create cache with test instrument
        let mut instruments_cache = AHashMap::new();
        let test_instrument = create_test_perpetual_instrument();
        instruments_cache.insert(Ustr::from("XBTUSD"), test_instrument);

        let updates = parse_instrument_msg(msg, &instruments_cache, UnixNanos::from(1));
        assert_eq!(updates.len(), 0);
    }

    #[rstest]
    fn test_parse_instrument_msg_index_symbol() {
        // Test for index symbols like .BXBT where lastPrice is the index price
        // and markPrice equals lastPrice
        let mut msg: BitmexInstrumentMsg =
            serde_json::from_str(&load_test_json("ws_instrument.json")).unwrap();
        msg.symbol = Ustr::from(".BXBT");
        msg.last_price = Some(119163.05);
        msg.mark_price = Some(119163.05); // Index symbols have mark price equal to last price
        msg.index_price = None;

        // Create instruments cache with proper precision for .BXBT
        let instrument_id = InstrumentId::from(".BXBT.BITMEX");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from(".BXBT"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false, // is_inverse
            2,     // price_precision (for 119163.05)
            8,     // size_precision
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,                 // multiplier
            None,                 // lot_size
            None,                 // max_quantity
            None,                 // min_quantity
            None,                 // max_notional
            None,                 // min_notional
            None,                 // max_price
            None,                 // min_price
            None,                 // margin_init
            None,                 // margin_maint
            None,                 // maker_fee
            None,                 // taker_fee
            UnixNanos::default(), // ts_event
            UnixNanos::default(), // ts_init
        );
        let mut instruments_cache = AHashMap::new();
        instruments_cache.insert(
            Ustr::from(".BXBT"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let updates = parse_instrument_msg(msg, &instruments_cache, UnixNanos::from(1));

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
                assert_eq!(update.ts_init, UnixNanos::from(1));
            }
            _ => panic!("Expected IndexPriceUpdate for index symbol"),
        }
    }

    #[rstest]
    fn test_parse_funding_msg() {
        let json_data = load_test_json("ws_funding_rate.json");
        let msg: BitmexFundingMsg = serde_json::from_str(&json_data).unwrap();
        let update = parse_funding_msg(msg, UnixNanos::from(1));

        assert!(update.is_some());
        let update = update.unwrap();

        assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(update.rate.to_string(), "0.0001");
        assert!(update.next_funding_ns.is_none());
        assert_eq!(update.ts_event, UnixNanos::from(1732507200000000000));
        assert_eq!(update.ts_init, UnixNanos::from(1));
    }
}
