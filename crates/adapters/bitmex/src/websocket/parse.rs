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

//! Parsers that convert BitMEX WebSocket payloads into Nautilus data structures.

use std::{num::NonZero, str::FromStr};

use ahash::AHashMap;
use chrono::Timelike;
use nautilus_core::{UnixNanos, uuid::UUID4};
#[cfg(test)]
use nautilus_model::types::Currency;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
        depth::DEPTH10_LEN,
    },
    enums::{
        AccountType, AggregationSource, BarAggregation, OrderSide, OrderStatus, OrderType,
        PriceType, RecordFlag, TimeInForce, TrailingOffsetType,
    },
    events::{
        OrderAccepted, OrderCanceled, OrderExpired, OrderRejected, OrderTriggered, OrderUpdated,
        account::state::AccountState,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    enums::{BitmexAction, BitmexWsTopic},
    messages::{
        BitmexExecutionMsg, BitmexFundingMsg, BitmexInstrumentMsg, BitmexMarginMsg,
        BitmexOrderBook10Msg, BitmexOrderBookMsg, BitmexOrderMsg, BitmexPositionMsg,
        BitmexQuoteMsg, BitmexTradeBinMsg, BitmexTradeMsg, BitmexWalletMsg,
    },
};
use crate::{
    common::{
        consts::BITMEX_VENUE,
        enums::{
            BitmexExecInstruction, BitmexExecType, BitmexOrderStatus, BitmexOrderType,
            BitmexPegPriceType, BitmexSide,
        },
        parse::{
            bitmex_currency_divisor, clean_reason, derive_trade_id, extract_trigger_type,
            map_bitmex_currency, normalize_trade_bin_prices, normalize_trade_bin_volume,
            parse_account_balance, parse_contracts_quantity, parse_fractional_quantity,
            parse_instrument_id, parse_liquidity_side, parse_optional_datetime_to_unix_nanos,
            parse_position_side, parse_signed_contracts_quantity,
        },
    },
    http::parse::get_currency,
    websocket::messages::BitmexOrderUpdateMsg,
};

const BAR_SPEC_1_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(1).expect("1 is a valid non-zero usize"),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_5_MINUTE: BarSpecification = BarSpecification {
    step: NonZero::new(5).expect("5 is a valid non-zero usize"),
    aggregation: BarAggregation::Minute,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_HOUR: BarSpecification = BarSpecification {
    step: NonZero::new(1).expect("1 is a valid non-zero usize"),
    aggregation: BarAggregation::Hour,
    price_type: PriceType::Last,
};
const BAR_SPEC_1_DAY: BarSpecification = BarSpecification {
    step: NonZero::new(1).expect("1 is a valid non-zero usize"),
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

/// Converts a batch of BitMEX order-book rows into Nautilus delta events.
#[must_use]
pub fn parse_book_msg_vec(
    data: Vec<BitmexOrderBookMsg>,
    action: BitmexAction,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut deltas = Vec::with_capacity(data.len());

    for msg in data {
        if let Some(instrument) = instruments.get(&msg.symbol) {
            let instrument_id = instrument.id();
            let price_precision = instrument.price_precision();
            deltas.push(Data::Delta(parse_book_msg(
                &msg,
                &action,
                instrument,
                instrument_id,
                price_precision,
                ts_init,
            )));
        } else {
            log::error!(
                "Instrument cache miss: book delta dropped for symbol={}",
                msg.symbol
            );
        }
    }

    // Set F_LAST on the last delta so data engine knows the batch is complete
    if let Some(Data::Delta(last_delta)) = deltas.last_mut() {
        *last_delta = OrderBookDelta::new(
            last_delta.instrument_id,
            last_delta.action,
            last_delta.order,
            last_delta.flags | RecordFlag::F_LAST as u8,
            last_delta.sequence,
            last_delta.ts_event,
            last_delta.ts_init,
        );
    }

    deltas
}

/// Converts BitMEX level-10 snapshots into Nautilus depth events.
#[must_use]
pub fn parse_book10_msg_vec(
    data: Vec<BitmexOrderBook10Msg>,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut depths = Vec::with_capacity(data.len());

    for msg in data {
        if let Some(instrument) = instruments.get(&msg.symbol) {
            let instrument_id = instrument.id();
            let price_precision = instrument.price_precision();
            match parse_book10_msg(&msg, instrument, instrument_id, price_precision, ts_init) {
                Ok(depth) => depths.push(Data::Depth10(Box::new(depth))),
                Err(e) => {
                    log::error!("Failed to parse orderBook10 for symbol={}: {e}", msg.symbol);
                }
            }
        } else {
            log::error!(
                "Instrument cache miss: depth10 message dropped for symbol={}",
                msg.symbol
            );
        }
    }
    depths
}

/// Converts BitMEX trade messages into Nautilus trade data events.
#[must_use]
pub fn parse_trade_msg_vec(
    data: Vec<BitmexTradeMsg>,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());

    for msg in data {
        if let Some(instrument) = instruments.get(&msg.symbol) {
            let instrument_id = instrument.id();
            let price_precision = instrument.price_precision();
            trades.push(Data::Trade(parse_trade_msg(
                &msg,
                instrument,
                instrument_id,
                price_precision,
                ts_init,
            )));
        } else {
            log::error!(
                "Instrument cache miss: trade message dropped for symbol={}",
                msg.symbol
            );
        }
    }
    trades
}

/// Converts aggregated trade-bin messages into Nautilus data events.
#[must_use]
pub fn parse_trade_bin_msg_vec(
    data: Vec<BitmexTradeBinMsg>,
    topic: &BitmexWsTopic,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) -> Vec<Data> {
    let mut trades = Vec::with_capacity(data.len());

    for msg in data {
        if let Some(instrument) = instruments.get(&msg.symbol) {
            let instrument_id = instrument.id();
            let price_precision = instrument.price_precision();
            trades.push(Data::Bar(parse_trade_bin_msg(
                &msg,
                topic,
                instrument,
                instrument_id,
                price_precision,
                ts_init,
            )));
        } else {
            log::error!(
                "Instrument cache miss: trade bin (bar) dropped for symbol={}",
                msg.symbol
            );
        }
    }
    trades
}

/// Converts a BitMEX order book row into a Nautilus order-book delta.
#[must_use]
pub fn parse_book_msg(
    msg: &BitmexOrderBookMsg,
    action: &BitmexAction,
    instrument: &InstrumentAny,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> OrderBookDelta {
    let flags = if action == &BitmexAction::Partial {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let action = action.as_book_action();
    let price = Price::new(msg.price, price_precision);
    let side = msg.side.as_order_side();
    let size = parse_contracts_quantity(msg.size.unwrap_or(0), instrument);
    let order_id = msg.id;
    let order = BookOrder::new(side, price, size, order_id);
    let sequence = 0; // Not available
    let ts_event = UnixNanos::from(msg.timestamp);

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
/// # Errors
///
/// Returns an error if the bid or ask arrays are not exactly 10 elements.
pub fn parse_book10_msg(
    msg: &BitmexOrderBook10Msg,
    instrument: &InstrumentAny,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDepth10> {
    let mut bids = Vec::with_capacity(DEPTH10_LEN);
    let mut asks = Vec::with_capacity(DEPTH10_LEN);

    // Initialized with zeros
    let mut bid_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];
    let mut ask_counts: [u32; DEPTH10_LEN] = [0; DEPTH10_LEN];

    for (i, level) in msg.bids.iter().enumerate() {
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::new(level[0], price_precision),
            parse_fractional_quantity(level[1], instrument),
            0,
        );

        bids.push(bid_order);
        bid_counts[i] = 1;
    }

    for (i, level) in msg.asks.iter().enumerate() {
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::new(level[0], price_precision),
            parse_fractional_quantity(level[1], instrument),
            0,
        );

        asks.push(ask_order);
        ask_counts[i] = 1;
    }

    let bids: [BookOrder; DEPTH10_LEN] = bids.try_into().map_err(|v: Vec<BookOrder>| {
        anyhow::anyhow!(
            "Bids length mismatch: expected {DEPTH10_LEN}, was {}",
            v.len()
        )
    })?;
    let asks: [BookOrder; DEPTH10_LEN] = asks.try_into().map_err(|v: Vec<BookOrder>| {
        anyhow::anyhow!(
            "Asks length mismatch: expected {DEPTH10_LEN}, was {}",
            v.len()
        )
    })?;

    let ts_event = UnixNanos::from(msg.timestamp);

    Ok(OrderBookDepth10::new(
        instrument_id,
        bids,
        asks,
        bid_counts,
        ask_counts,
        RecordFlag::F_SNAPSHOT as u8,
        0, // Not applicable for BitMEX L2 books
        ts_event,
        ts_init,
    ))
}

/// Converts a BitMEX quote message into a `QuoteTick`, filling missing data from cache.
#[must_use]
pub fn parse_quote_msg(
    msg: &BitmexQuoteMsg,
    last_quote: &QuoteTick,
    instrument: &InstrumentAny,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> QuoteTick {
    let bid_price = match msg.bid_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.bid_price,
    };

    let ask_price = match msg.ask_price {
        Some(price) => Price::new(price, price_precision),
        None => last_quote.ask_price,
    };

    let bid_size = match msg.bid_size {
        Some(size) => parse_contracts_quantity(size, instrument),
        None => last_quote.bid_size,
    };

    let ask_size = match msg.ask_size {
        Some(size) => parse_contracts_quantity(size, instrument),
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

/// Converts a BitMEX trade message into a `TradeTick`.
#[must_use]
pub fn parse_trade_msg(
    msg: &BitmexTradeMsg,
    instrument: &InstrumentAny,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> TradeTick {
    let price = Price::new(msg.price, price_precision);
    let size = parse_contracts_quantity(msg.size, instrument);
    let aggressor_side = msg.side.as_aggressor_side();
    let ts_event = UnixNanos::from(msg.timestamp);
    let trade_id = match msg.trd_match_id {
        Some(uuid) => TradeId::new(uuid.to_string()),
        None => derive_trade_id(
            msg.symbol,
            ts_event.as_u64(),
            msg.price,
            msg.size as i64,
            Some(msg.side.into()),
        ),
    };

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

/// Converts a BitMEX trade-bin summary into a `Bar` for the matching topic.
#[must_use]
pub fn parse_trade_bin_msg(
    msg: &BitmexTradeBinMsg,
    topic: &BitmexWsTopic,
    instrument: &InstrumentAny,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> Bar {
    let spec = bar_spec_from_topic(topic);
    let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

    let open = Price::new(msg.open, price_precision);
    let high = Price::new(msg.high, price_precision);
    let low = Price::new(msg.low, price_precision);
    let close = Price::new(msg.close, price_precision);

    let (open, high, low, close) =
        normalize_trade_bin_prices(open, high, low, close, &msg.symbol, Some(&bar_type));

    let volume_contracts = normalize_trade_bin_volume(Some(msg.volume), &msg.symbol);
    let volume = parse_contracts_quantity(volume_contracts, instrument);
    let ts_event = UnixNanos::from(msg.timestamp);

    Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Converts a WebSocket topic to a bar specification.
///
/// Returns `BAR_SPEC_1_MINUTE` and logs an error for unsupported topics.
#[must_use]
pub fn bar_spec_from_topic(topic: &BitmexWsTopic) -> BarSpecification {
    match topic {
        BitmexWsTopic::TradeBin1m => BAR_SPEC_1_MINUTE,
        BitmexWsTopic::TradeBin5m => BAR_SPEC_5_MINUTE,
        BitmexWsTopic::TradeBin1h => BAR_SPEC_1_HOUR,
        BitmexWsTopic::TradeBin1d => BAR_SPEC_1_DAY,
        _ => {
            log::error!("Bar specification not supported: topic={topic:?}");
            BAR_SPEC_1_MINUTE
        }
    }
}

/// Converts a bar specification to a WebSocket topic.
///
/// Returns `TradeBin1m` and logs an error for unsupported specifications.
#[must_use]
pub fn topic_from_bar_spec(spec: BarSpecification) -> BitmexWsTopic {
    match spec {
        BAR_SPEC_1_MINUTE => BitmexWsTopic::TradeBin1m,
        BAR_SPEC_5_MINUTE => BitmexWsTopic::TradeBin5m,
        BAR_SPEC_1_HOUR => BitmexWsTopic::TradeBin1h,
        BAR_SPEC_1_DAY => BitmexWsTopic::TradeBin1d,
        _ => {
            log::error!("Bar specification not supported: spec={spec:?}");
            BitmexWsTopic::TradeBin1m
        }
    }
}

fn infer_order_type_from_msg(msg: &BitmexOrderMsg) -> OrderType {
    if msg.stop_px.is_some() {
        if msg.price.is_some() {
            OrderType::StopLimit
        } else {
            OrderType::StopMarket
        }
    } else if msg.price.is_some() {
        OrderType::Limit
    } else {
        OrderType::Market
    }
}

/// Parse a BitMEX WebSocket order message into a Nautilus `OrderStatusReport`.
///
/// # References
///
/// <https://www.bitmex.com/app/wsAPI#Order>
///
/// # Errors
///
/// Returns an error if the time in force conversion fails.
pub fn parse_order_msg(
    msg: &BitmexOrderMsg,
    instrument: &InstrumentAny,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account)); // TODO: Revisit
    let instrument_id = parse_instrument_id(msg.symbol);
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let common_side: BitmexSide = msg.side.into();
    let order_side: OrderSide = common_side.into();

    let order_type: OrderType = if let Some(ord_type) = msg.ord_type {
        // Pegged orders with TrailingStopPeg are trailing stop orders
        if ord_type == BitmexOrderType::Pegged
            && msg.peg_price_type == Some(BitmexPegPriceType::TrailingStopPeg)
        {
            if msg.price.is_some() {
                OrderType::TrailingStopLimit
            } else {
                OrderType::TrailingStopMarket
            }
        } else {
            ord_type.into()
        }
    } else if let Some(client_order_id) = msg.cl_ord_id {
        let client_order_id = ClientOrderId::new(client_order_id);
        if let Some(&cached) = order_type_cache.get(&client_order_id) {
            cached
        } else {
            let inferred = infer_order_type_from_msg(msg);
            order_type_cache.insert(client_order_id, inferred);
            inferred
        }
    } else {
        infer_order_type_from_msg(msg)
    };

    let time_in_force: TimeInForce = match msg.time_in_force {
        Some(tif) => tif.try_into().map_err(|e| anyhow::anyhow!("{e}"))?,
        None => TimeInForce::Gtc,
    };
    let order_status: OrderStatus = msg.ord_status.into();
    let quantity = parse_signed_contracts_quantity(msg.order_qty, instrument);
    let filled_qty = parse_signed_contracts_quantity(msg.cum_qty, instrument);
    let report_id = UUID4::new();
    let ts_accepted =
        parse_optional_datetime_to_unix_nanos(&Some(msg.transact_time), "transact_time");
    let ts_last = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "timestamp");

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
        report = report.with_price(Price::new(price, instrument.price_precision()));
    }

    if let Some(avg_px) = msg.avg_px {
        report = report.with_avg_px(avg_px)?;
    }

    if let Some(trigger_price) = msg.stop_px {
        report = report
            .with_trigger_price(Price::new(trigger_price, instrument.price_precision()))
            .with_trigger_type(extract_trigger_type(msg.exec_inst.as_ref()));
    }

    // Populate trailing offset for trailing stop orders
    if matches!(
        order_type,
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    ) && let Some(peg_offset) = msg.peg_offset_value
    {
        let trailing_offset = Decimal::try_from(peg_offset.abs())
            .unwrap_or_else(|_| Decimal::new(peg_offset.abs() as i64, 0));
        report = report
            .with_trailing_offset(trailing_offset)
            .with_trailing_offset_type(TrailingOffsetType::Price);

        if msg.stop_px.is_none() {
            report = report.with_trigger_type(extract_trigger_type(msg.exec_inst.as_ref()));
        }
    }

    if let Some(exec_insts) = &msg.exec_inst {
        for exec_inst in exec_insts {
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
    }

    // Extract rejection reason for rejected orders
    if order_status == OrderStatus::Rejected {
        if let Some(reason_str) = msg.ord_rej_reason.or(msg.text) {
            log::debug!(
                "Order rejected with reason: order_id={:?}, client_order_id={:?}, reason={:?}",
                venue_order_id,
                msg.cl_ord_id,
                reason_str,
            );
            report = report.with_cancel_reason(clean_reason(reason_str.as_ref()));
        } else {
            log::debug!(
                "Order rejected without reason from BitMEX: order_id={:?}, client_order_id={:?}, ord_status={:?}, ord_rej_reason={:?}, text={:?}",
                venue_order_id,
                msg.cl_ord_id,
                msg.ord_status,
                msg.ord_rej_reason,
                msg.text,
            );
        }
    }

    // Check if this is a canceled post-only order (BitMEX cancels instead of rejecting)
    // We need to preserve the rejection reason for the execution client to handle
    if order_status == OrderStatus::Canceled
        && let Some(reason_str) = msg.ord_rej_reason.or(msg.text)
    {
        report = report.with_cancel_reason(clean_reason(reason_str.as_ref()));
    }

    Ok(report)
}

/// Parsed order event variants produced by [`parse_order_event`] for tracked orders.
#[derive(Debug, Clone)]
pub enum ParsedOrderEvent {
    Accepted(OrderAccepted),
    Canceled(OrderCanceled),
    Expired(OrderExpired),
    Triggered(OrderTriggered),
    Rejected(OrderRejected),
}

/// Converts a full BitMEX order message into a [`ParsedOrderEvent`] for tracked orders.
///
/// Returns `None` for transitional statuses (`PendingNew`, `PendingCancel`, `PendingReplace`)
/// and for fill-related statuses (`PartiallyFilled`, `Filled`, `Rejected`) that are handled
/// through other channels (Execution table for fills, HTTP response for rejections).
pub fn parse_order_event(
    msg: &BitmexOrderMsg,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    trader_id: TraderId,
    strategy_id: StrategyId,
    ts_init: UnixNanos,
) -> Option<ParsedOrderEvent> {
    let instrument_id = parse_instrument_id(msg.symbol);
    let venue_order_id = VenueOrderId::new(msg.order_id.to_string());
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "timestamp");

    match msg.ord_status {
        BitmexOrderStatus::New => {
            let accepted = OrderAccepted::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
            );
            Some(ParsedOrderEvent::Accepted(accepted))
        }
        BitmexOrderStatus::Canceled => {
            // BitMEX cancels post-only orders instead of rejecting them when they
            // would cross the spread. Detect via "ParticipateDoNotInitiate" reason.
            let cancel_reason = msg
                .ord_rej_reason
                .or(msg.text)
                .map(|r| clean_reason(r.as_ref()));

            let is_post_only_rejection = cancel_reason
                .as_deref()
                .is_some_and(|r| r.contains("ParticipateDoNotInitiate"));

            if is_post_only_rejection {
                let rejected = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    Ustr::from(
                        cancel_reason
                            .as_deref()
                            .unwrap_or("Post-only order rejected"),
                    ),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    true, // due_post_only
                );
                Some(ParsedOrderEvent::Rejected(rejected))
            } else {
                let canceled = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                Some(ParsedOrderEvent::Canceled(canceled))
            }
        }
        BitmexOrderStatus::Expired => {
            let expired = OrderExpired::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(venue_order_id),
                Some(account_id),
            );
            Some(ParsedOrderEvent::Expired(expired))
        }
        // Rejections: handled at submit time via HTTP response
        // Fills: handled via the Execution table, not order status updates
        // Transitional: PendingNew, PendingCancel, PendingReplace
        _ => None,
    }
}

/// Parse a BitMEX WebSocket order update message into a Nautilus `OrderUpdated` event.
///
/// This handles partial updates where only changed fields are present.
pub fn parse_order_update_msg(
    msg: &BitmexOrderUpdateMsg,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> Option<OrderUpdated> {
    // Uses external IDs; callers enrich with tracked identity when available
    let trader_id = TraderId::external();
    let strategy_id = StrategyId::external();
    let instrument_id = parse_instrument_id(msg.symbol);
    let venue_order_id = Some(VenueOrderId::new(msg.order_id.to_string()));
    let client_order_id = msg
        .cl_ord_id
        .as_ref()
        .map_or_else(ClientOrderId::external, ClientOrderId::new);

    // BitMEX partial updates may omit leaves_qty/cum_qty. When missing, we fall back
    // to zero which signals the execution engine to use the cached order quantity.
    let quantity = match (msg.leaves_qty, msg.cum_qty) {
        (Some(leaves), Some(cum)) => parse_contracts_quantity((leaves + cum) as u64, instrument),
        _ => Quantity::zero(instrument.size_precision()),
    };
    let price = msg
        .price
        .map(|p| Price::new(p, instrument.price_precision()));

    // BitMEX doesn't send trigger price in regular order updates?
    let trigger_price = None;
    // BitMEX doesn't send protection price in regular order updates
    let protection_price = None;

    let event_id = UUID4::new();
    let ts_event = parse_optional_datetime_to_unix_nanos(&msg.timestamp, "timestamp");

    Some(OrderUpdated::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        quantity,
        event_id,
        ts_event,
        ts_init,
        false, // reconciliation
        venue_order_id,
        Some(account_id),
        price,
        trigger_price,
        protection_price,
        false, // is_quote_quantity
    ))
}

/// Parse a BitMEX WebSocket execution message into a Nautilus `FillReport`.
///
/// Handles different execution types appropriately:
/// - `Trade`: Normal trade execution → FillReport
/// - `Liquidation`: Auto-deleveraging or liquidation → FillReport
/// - `Bankruptcy`: Bankruptcy execution → FillReport (with warning)
/// - `Settlement`, `TrialFill`: Non-obvious cases → None (with warning)
/// - `Funding`, `Insurance`, `Rebalance`: Expected non-fills → None (debug log)
/// - Order state changes (`New`, `Canceled`, etc.): → None (debug log)
///
/// # References
///
/// <https://www.bitmex.com/app/wsAPI#Execution>
pub fn parse_execution_msg(
    msg: BitmexExecutionMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Option<FillReport> {
    let exec_type = msg.exec_type?;

    match exec_type {
        // Position-affecting executions that generate fills
        BitmexExecType::Trade | BitmexExecType::Liquidation => {}
        BitmexExecType::Bankruptcy => {
            log::warn!(
                "Processing bankruptcy execution as fill: exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
        }

        // Settlement executions are mark-to-market events, not fills
        BitmexExecType::Settlement => {
            log::debug!(
                "Settlement execution skipped (not a fill): applies quanto conversion/PnL transfer on contract settlement: exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }
        BitmexExecType::TrialFill => {
            log::warn!(
                "Trial fill execution received (testnet only), not processed as fill: exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }

        // Expected non-fill executions
        BitmexExecType::Funding => {
            log::debug!(
                "Funding execution skipped (not a fill): exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }
        BitmexExecType::Insurance => {
            log::debug!(
                "Insurance execution skipped (not a fill): exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }
        BitmexExecType::Rebalance => {
            log::debug!(
                "Rebalance execution skipped (not a fill): exec_type={exec_type:?}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }

        // Order state changes (not fills)
        BitmexExecType::New
        | BitmexExecType::Canceled
        | BitmexExecType::CancelReject
        | BitmexExecType::Replaced
        | BitmexExecType::Rejected
        | BitmexExecType::AmendReject
        | BitmexExecType::Suspended
        | BitmexExecType::Released
        | BitmexExecType::TriggeredOrActivatedBySystem => {
            log::debug!(
                "Execution message skipped (order state change, not a fill): exec_type={exec_type:?}, order_id={:?}",
                msg.order_id,
            );
            return None;
        }

        BitmexExecType::Unknown(ref type_str) => {
            log::warn!(
                "Unknown execution type received, skipping: exec_type={type_str}, order_id={:?}, symbol={:?}",
                msg.order_id,
                msg.symbol,
            );
            return None;
        }
    }

    let account_id = AccountId::new(format!("BITMEX-{}", msg.account?));
    let instrument_id = parse_instrument_id(msg.symbol?);
    let venue_order_id = VenueOrderId::new(msg.order_id?.to_string());
    let trade_id = TradeId::new(msg.trd_match_id?.to_string());
    let order_side: OrderSide = msg.side.map_or(OrderSide::NoOrderSide, |s| {
        let side: BitmexSide = s.into();
        side.into()
    });
    let last_qty = parse_signed_contracts_quantity(msg.last_qty?, instrument);
    let last_px = Price::new(msg.last_px?, instrument.price_precision());
    let settlement_currency_str = msg.settl_currency.unwrap_or(Ustr::from("XBT"));
    let mapped_currency = map_bitmex_currency(settlement_currency_str.as_str());
    let currency = get_currency(&mapped_currency);
    let commission = Money::new(msg.commission.unwrap_or(0.0), currency);
    let liquidity_side = parse_liquidity_side(&msg.last_liquidity_ind);
    let client_order_id = msg.cl_ord_id.map(ClientOrderId::new);
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_event = parse_optional_datetime_to_unix_nanos(&msg.transact_time, "transact_time");

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
pub fn parse_position_msg(
    msg: &BitmexPositionMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> PositionStatusReport {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let instrument_id = parse_instrument_id(msg.symbol);
    let position_side = parse_position_side(msg.current_qty).as_specified();
    let quantity = parse_signed_contracts_quantity(msg.current_qty.unwrap_or(0), instrument);
    let venue_position_id = None; // Not applicable on BitMEX
    let avg_px_open = msg
        .avg_entry_price
        .and_then(|p| Decimal::from_str(&p.to_string()).ok());
    let ts_last = parse_optional_datetime_to_unix_nanos(&msg.timestamp, "timestamp");

    PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None,              // report_id
        venue_position_id, // venue_position_id
        avg_px_open,       // avg_px_open
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
    msg: &BitmexInstrumentMsg,
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
                log::trace!(
                    "Index instrument {} not in cache, skipping update",
                    msg.symbol
                );
            } else {
                log::debug!("Instrument {} not in cache, skipping update", msg.symbol);
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
/// Returns `FundingRateUpdate` containing funding rate information.
/// Note: This returns `FundingRateUpdate` directly, not wrapped in Data enum,
/// to keep it separate from the FFI layer.
#[must_use]
pub fn parse_funding_msg(msg: &BitmexFundingMsg, ts_init: UnixNanos) -> FundingRateUpdate {
    let instrument_id = InstrumentId::from(format!("{}.BITMEX", msg.symbol));
    let interval_hours = msg.funding_interval.hour();
    let interval_minutes = msg.funding_interval.minute();
    let interval = Some((interval_hours * 60 + interval_minutes) as u16);
    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "");

    FundingRateUpdate::new(
        instrument_id,
        msg.funding_rate,
        interval,
        None, // Next funding time not provided in this message
        ts_event,
        ts_init,
    )
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
pub fn parse_wallet_msg(msg: &BitmexWalletMsg, ts_init: UnixNanos) -> AccountState {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));

    // Map BitMEX currency to standard currency code
    let currency_str = map_bitmex_currency(msg.currency.as_str());
    let currency = get_currency(&currency_str);

    // Wallet messages do not expose locked margin; treat the full balance as free
    // and let the centralized helper enforce `total == locked + free` at currency precision.
    let divisor = bitmex_currency_divisor(msg.currency.as_str());
    let amount_dec = Decimal::from(msg.amount.unwrap_or(0)) / divisor;

    let balance = AccountBalance::from_total_and_locked(amount_dec, Decimal::ZERO, currency)
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

/// Parse a BitMEX margin message into an account-wide [`MarginBalance`].
#[must_use]
pub fn parse_margin_msg(msg: &BitmexMarginMsg) -> MarginBalance {
    let currency_str = map_bitmex_currency(msg.currency.as_str());
    let currency = get_currency(&currency_str);

    let divisor = bitmex_currency_divisor(msg.currency.as_str());
    let initial_dec = Decimal::from(msg.init_margin.unwrap_or(0).max(0)) / divisor;
    let maintenance_dec = Decimal::from(msg.maint_margin.unwrap_or(0).max(0)) / divisor;

    MarginBalance::new(
        Money::from_decimal(initial_dec, currency).unwrap_or_else(|_| Money::zero(currency)),
        Money::from_decimal(maintenance_dec, currency).unwrap_or_else(|_| Money::zero(currency)),
        None,
    )
}

/// Parses a BitMEX margin message into an [`AccountState`] with balances and margins.
#[must_use]
pub fn parse_margin_account_state(msg: &BitmexMarginMsg, ts_init: UnixNanos) -> AccountState {
    let account_id = AccountId::new(format!("BITMEX-{}", msg.account));
    let balance = parse_account_balance(msg);

    let margin = parse_margin_msg(msg);

    let margins = if !margin.initial.is_zero() || !margin.maintenance.is_zero() {
        vec![margin]
    } else {
        vec![]
    };

    let ts_event = parse_optional_datetime_to_unix_nanos(&Some(msg.timestamp), "margin.timestamp");

    AccountState::new(
        account_id,
        AccountType::Margin,
        vec![balance],
        margins,
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    )
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use nautilus_model::{
        enums::{AggressorSide, BookAction, LiquiditySide, PositionSide},
        identifiers::Symbol,
        instruments::crypto_perpetual::CryptoPerpetual,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::common::{
        enums::{BitmexExecType, BitmexOrderStatus},
        testing::load_test_json,
    };

    // Helper function to create a test perpetual instrument for tests
    fn create_test_perpetual_instrument_with_precisions(
        price_precision: u8,
        size_precision: u8,
    ) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::from("XBTUSD.BITMEX"),
            Symbol::new("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::BTC(),
            true, // is_inverse
            price_precision,
            size_precision,
            Price::new(0.5, price_precision),
            Quantity::new(1.0, size_precision),
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
            None, // info
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn create_test_perpetual_instrument() -> InstrumentAny {
        create_test_perpetual_instrument_with_precisions(1, 0)
    }

    #[rstest]
    fn test_orderbook_l2_message() {
        let json_data = load_test_json("ws_orderbook_l2.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: BitmexOrderBookMsg = serde_json::from_str(&json_data).unwrap();

        // Test Insert action
        let instrument = create_test_perpetual_instrument();

        // Test Insert action (no snapshot flag)
        let delta = parse_book_msg(
            &msg,
            &BitmexAction::Insert,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );
        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.order.price, Price::from("98459.9"));
        assert_eq!(delta.order.size, Quantity::from(33000));
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.order.order_id, 62400580205);
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.sequence, 0);
        assert_eq!(delta.ts_event, 1732436782356000000); // 2024-11-24T08:26:22.356Z in nanos
        assert_eq!(delta.ts_init, 3);

        // Test Partial action (should have F_SNAPSHOT flag)
        let delta = parse_book_msg(
            &msg,
            &BitmexAction::Partial,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );
        assert_eq!(delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(delta.action, BookAction::Add);

        // Test Update action (no flags)
        let delta = parse_book_msg(
            &msg,
            &BitmexAction::Update,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.action, BookAction::Update);
    }

    #[rstest]
    fn test_orderbook10_message() {
        let json_data = load_test_json("ws_orderbook_10.json");
        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let msg: BitmexOrderBook10Msg = serde_json::from_str(&json_data).unwrap();
        let instrument = create_test_perpetual_instrument();
        let depth10 = parse_book10_msg(
            &msg,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        )
        .unwrap();

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
        let instrument = create_test_perpetual_instrument_with_precisions(2, 0);
        let quote = parse_quote_msg(
            &msg,
            &last_quote,
            &instrument,
            instrument_id,
            instrument.price_precision(),
            UnixNanos::from(3),
        );

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
        let instrument = create_test_perpetual_instrument();
        let trade = parse_trade_msg(
            &msg,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );

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
    fn test_trade_message_derives_trade_id_when_trd_match_id_missing() {
        let json_data = load_test_json("ws_trade.json");
        let mut msg: BitmexTradeMsg = serde_json::from_str(&json_data).unwrap();
        msg.trd_match_id = None;
        let instrument = create_test_perpetual_instrument();

        let trade = parse_trade_msg(
            &msg,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );

        let mut again_msg: BitmexTradeMsg = serde_json::from_str(&json_data).unwrap();
        again_msg.trd_match_id = None;
        let again = parse_trade_msg(
            &again_msg,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );

        assert_eq!(trade.trade_id, again.trade_id, "derivation must be stable");
        assert_eq!(trade.trade_id.as_str().len(), 16);

        let mut altered: BitmexTradeMsg = serde_json::from_str(&json_data).unwrap();
        altered.trd_match_id = None;
        altered.price += 1.0;
        let altered_trade = parse_trade_msg(
            &altered,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );
        assert_ne!(trade.trade_id, altered_trade.trade_id);
    }

    #[rstest]
    fn test_trade_bin_message() {
        let json_data = load_test_json("ws_trade_bin_1m.json");

        let instrument_id = InstrumentId::from("XBTUSD.BITMEX");
        let topic = BitmexWsTopic::TradeBin1m;

        let msg: BitmexTradeBinMsg = serde_json::from_str(&json_data).unwrap();
        let instrument = create_test_perpetual_instrument();
        let bar = parse_trade_bin_msg(
            &msg,
            &topic,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );

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
    fn test_trade_bin_message_extreme_adjustment() {
        let topic = BitmexWsTopic::TradeBin1m;
        let instrument = create_test_perpetual_instrument();

        let msg = BitmexTradeBinMsg {
            timestamp: DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            symbol: Ustr::from("XBTUSD"),
            open: 50_000.0,
            high: 49_990.0,
            low: 50_010.0,
            close: 50_005.0,
            trades: 10,
            volume: 1_000,
            vwap: Some(0.0),
            last_size: Some(0),
            turnover: 0,
            home_notional: 0.0,
            foreign_notional: 0.0,
            pool: None,
        };

        let bar = parse_trade_bin_msg(
            &msg,
            &topic,
            &instrument,
            instrument.id(),
            instrument.price_precision(),
            UnixNanos::from(3),
        );

        assert_eq!(bar.high, Price::from("50010.0"));
        assert_eq!(bar.low, Price::from("49990.0"));
        assert_eq!(bar.open, Price::from("50000.0"));
        assert_eq!(bar.close, Price::from("50005.0"));
        assert_eq!(bar.volume, Quantity::from(1_000));
    }

    #[rstest]
    fn test_parse_order_msg() {
        let json_data = load_test_json("ws_order.json");
        let msg: BitmexOrderMsg = serde_json::from_str(&json_data).unwrap();
        let mut cache = AHashMap::new();
        let instrument = create_test_perpetual_instrument();
        let report = parse_order_msg(&msg, &instrument, &mut cache, UnixNanos::default()).unwrap();

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
    fn test_parse_order_msg_infers_type_when_missing() {
        let json_data = load_test_json("ws_order.json");
        let mut msg: BitmexOrderMsg = serde_json::from_str(&json_data).unwrap();
        msg.ord_type = None;
        msg.cl_ord_id = None;
        msg.price = Some(98_000.0);
        msg.stop_px = None;

        let mut cache = AHashMap::new();
        let instrument = create_test_perpetual_instrument();

        let report = parse_order_msg(&msg, &instrument, &mut cache, UnixNanos::default()).unwrap();

        assert_eq!(report.order_type, OrderType::Limit);
    }

    #[rstest]
    fn test_parse_order_msg_rejected_with_reason() {
        let mut msg: BitmexOrderMsg =
            serde_json::from_str(&load_test_json("ws_order.json")).unwrap();
        msg.ord_status = BitmexOrderStatus::Rejected;
        msg.ord_rej_reason = Some(Ustr::from("Insufficient available balance"));
        msg.text = None;
        msg.cum_qty = 0;

        let mut cache = AHashMap::new();
        let instrument = create_test_perpetual_instrument();
        let report = parse_order_msg(&msg, &instrument, &mut cache, UnixNanos::default()).unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(
            report.cancel_reason,
            Some("Insufficient available balance".to_string())
        );
    }

    #[rstest]
    fn test_parse_order_msg_rejected_with_text_fallback() {
        let mut msg: BitmexOrderMsg =
            serde_json::from_str(&load_test_json("ws_order.json")).unwrap();
        msg.ord_status = BitmexOrderStatus::Rejected;
        msg.ord_rej_reason = None;
        msg.text = Some(Ustr::from("Order would execute immediately"));
        msg.cum_qty = 0;

        let mut cache = AHashMap::new();
        let instrument = create_test_perpetual_instrument();
        let report = parse_order_msg(&msg, &instrument, &mut cache, UnixNanos::default()).unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(
            report.cancel_reason,
            Some("Order would execute immediately".to_string())
        );
    }

    #[rstest]
    fn test_parse_order_msg_rejected_without_reason() {
        let mut msg: BitmexOrderMsg =
            serde_json::from_str(&load_test_json("ws_order.json")).unwrap();
        msg.ord_status = BitmexOrderStatus::Rejected;
        msg.ord_rej_reason = None;
        msg.text = None;
        msg.cum_qty = 0;

        let mut cache = AHashMap::new();
        let instrument = create_test_perpetual_instrument();
        let report = parse_order_msg(&msg, &instrument, &mut cache, UnixNanos::default()).unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(report.cancel_reason, None);
    }

    #[rstest]
    fn test_parse_execution_msg() {
        let json_data = load_test_json("ws_execution.json");
        let msg: BitmexExecutionMsg = serde_json::from_str(&json_data).unwrap();
        let instrument = create_test_perpetual_instrument();
        let fill = parse_execution_msg(msg, &instrument, UnixNanos::default()).unwrap();

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

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_cancel_reject_execution() {
        // Test that CancelReject messages can be parsed (even without symbol)
        let json = load_test_json("ws_execution_cancel_reject.json");

        let msg: BitmexExecutionMsg = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.exec_type, Some(BitmexExecType::CancelReject));
        assert_eq!(msg.ord_status, Some(BitmexOrderStatus::Rejected));
        assert_eq!(msg.symbol, None);

        // Should return None since it's not a Trade
        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_liquidation() {
        // Critical for ADL/hedge tracking
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Liquidation);

        let instrument = create_test_perpetual_instrument();
        let fill = parse_execution_msg(msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(fill.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(fill.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::from(100));
        assert_eq!(fill.last_px, Price::from("98950.0"));
    }

    #[rstest]
    fn test_parse_execution_msg_bankruptcy() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Bankruptcy);

        let instrument = create_test_perpetual_instrument();
        let fill = parse_execution_msg(msg, &instrument, UnixNanos::default()).unwrap();

        assert_eq!(fill.account_id.to_string(), "BITMEX-1234567");
        assert_eq!(fill.instrument_id, InstrumentId::from("XBTUSD.BITMEX"));
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::from(100));
    }

    #[rstest]
    fn test_parse_execution_msg_settlement() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Settlement);

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_trial_fill() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::TrialFill);

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_funding() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Funding);

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_insurance() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Insurance);

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_rebalance() {
        let mut msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        msg.exec_type = Some(BitmexExecType::Rebalance);

        let instrument = create_test_perpetual_instrument();
        let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_execution_msg_order_state_changes() {
        let instrument = create_test_perpetual_instrument();

        let order_state_types = vec![
            BitmexExecType::New,
            BitmexExecType::Canceled,
            BitmexExecType::CancelReject,
            BitmexExecType::Replaced,
            BitmexExecType::Rejected,
            BitmexExecType::AmendReject,
            BitmexExecType::Suspended,
            BitmexExecType::Released,
            BitmexExecType::TriggeredOrActivatedBySystem,
        ];

        for exec_type in order_state_types {
            let mut msg: BitmexExecutionMsg =
                serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
            msg.exec_type = Some(exec_type.clone());

            let result = parse_execution_msg(msg, &instrument, UnixNanos::default());
            assert!(
                result.is_none(),
                "Expected None for exec_type {exec_type:?}"
            );
        }
    }

    #[rstest]
    fn test_parse_position_msg() {
        let json_data = load_test_json("ws_position.json");
        let msg: BitmexPositionMsg = serde_json::from_str(&json_data).unwrap();
        let instrument = create_test_perpetual_instrument();
        let report = parse_position_msg(&msg, &instrument, UnixNanos::default());

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

        let instrument = create_test_perpetual_instrument();
        let report = parse_position_msg(&msg, &instrument, UnixNanos::default());
        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity, Quantity::from(500));
    }

    #[rstest]
    fn test_parse_position_msg_flat() {
        let mut msg: BitmexPositionMsg =
            serde_json::from_str(&load_test_json("ws_position.json")).unwrap();
        msg.current_qty = Some(0);

        let instrument = create_test_perpetual_instrument();
        let report = parse_position_msg(&msg, &instrument, UnixNanos::default());
        assert_eq!(report.position_side.as_position_side(), PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::from(0));
    }

    #[rstest]
    fn test_parse_wallet_msg() {
        let json_data = load_test_json("ws_wallet.json");
        let msg: BitmexWalletMsg = serde_json::from_str(&json_data).unwrap();
        let ts_init = UnixNanos::from(1);
        let account_state = parse_wallet_msg(&msg, ts_init);

        assert_eq!(account_state.account_id.to_string(), "BITMEX-1234567");
        assert!(!account_state.balances.is_empty());
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency.code.to_string(), "XBT");
        // Amount should be converted from satoshis (100005180 / 100_000_000.0 = 1.0000518)
        assert!((balance.total.as_f64() - 1.0000518).abs() < 1e-7);
        // Wallet messages do not carry locked margin; full amount is free.
        assert_eq!(balance.locked.as_f64(), 0.0);
        assert_eq!(balance.free.as_decimal(), balance.total.as_decimal());
    }

    #[rstest]
    fn test_parse_wallet_msg_no_amount() {
        let mut msg: BitmexWalletMsg =
            serde_json::from_str(&load_test_json("ws_wallet.json")).unwrap();
        msg.amount = None;

        let ts_init = UnixNanos::from(1);
        let account_state = parse_wallet_msg(&msg, ts_init);
        let balance = &account_state.balances[0];
        assert_eq!(balance.total.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_margin_msg() {
        let json_data = load_test_json("ws_margin.json");
        let msg: BitmexMarginMsg = serde_json::from_str(&json_data).unwrap();
        let margin_balance = parse_margin_msg(&msg);

        assert_eq!(margin_balance.currency.code.to_string(), "XBT");
        assert!(margin_balance.instrument_id.is_none());
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

        let margin_balance = parse_margin_msg(&msg);
        // Should still have valid margin values even if available_margin is None
        assert!(margin_balance.initial.as_f64() >= 0.0);
        assert!(margin_balance.maintenance.as_f64() >= 0.0);
    }

    #[rstest]
    fn test_parse_margin_account_state_includes_margins() {
        let msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("USDt"),
            risk_limit: None,
            amount: Some(5_000_000_000),
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: Some(200_000_000),  // 200 USDT
            maint_margin: Some(100_000_000), // 100 USDT
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(5_000_000_000), // 5000 USDT
            margin_balance: None,
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(4_800_000_000), // 4800 USDT
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let ts_init = UnixNanos::from(1_000_000_000u64);
        let state = parse_margin_account_state(&msg, ts_init);

        assert_eq!(state.account_id.to_string(), "BITMEX-123456");
        assert_eq!(state.account_type, AccountType::Margin);
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.margins.len(), 1);

        let balance = &state.balances[0];
        assert_eq!(balance.total.as_f64(), 5000.0);

        let margin = &state.margins[0];
        assert!(margin.instrument_id.is_none());
        assert_eq!(margin.currency.code.as_str(), "USDT");
        assert_eq!(margin.initial.as_f64(), 200.0);
        assert_eq!(margin.maintenance.as_f64(), 100.0);
    }

    #[rstest]
    fn test_parse_margin_account_state_zero_margins_excluded() {
        let msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: None,
            amount: Some(100_000_000),
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: Some(0),
            maint_margin: Some(0),
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(100_000_000),
            margin_balance: None,
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(100_000_000),
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let state = parse_margin_account_state(&msg, UnixNanos::from(1_000_000_000u64));

        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.margins.len(), 0);
    }

    #[rstest]
    fn test_parse_instrument_msg_both_prices() {
        let json_data = load_test_json("ws_instrument.json");
        let msg: BitmexInstrumentMsg = serde_json::from_str(&json_data).unwrap();

        // Create cache with test instrument
        let mut instruments_cache = AHashMap::new();
        let test_instrument = create_test_perpetual_instrument();
        instruments_cache.insert(Ustr::from("XBTUSD"), test_instrument);

        let updates = parse_instrument_msg(&msg, &instruments_cache, UnixNanos::from(1));

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

        let updates = parse_instrument_msg(&msg, &instruments_cache, UnixNanos::from(1));

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

        let updates = parse_instrument_msg(&msg, &instruments_cache, UnixNanos::from(1));

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

        let updates = parse_instrument_msg(&msg, &instruments_cache, UnixNanos::from(1));
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
            None,                 // info
            UnixNanos::default(), // ts_event
            UnixNanos::default(), // ts_init
        );
        let mut instruments_cache = AHashMap::new();
        instruments_cache.insert(
            Ustr::from(".BXBT"),
            InstrumentAny::CryptoPerpetual(instrument),
        );

        let updates = parse_instrument_msg(&msg, &instruments_cache, UnixNanos::from(1));

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
        let update = parse_funding_msg(&msg, UnixNanos::from(1));

        assert_eq!(update.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(update.rate.to_string(), "0.0001");
        assert_eq!(update.interval, Some(60 * 8));
        assert!(update.next_funding_ns.is_none());
        assert_eq!(update.ts_event, UnixNanos::from(1732507200000000000));
        assert_eq!(update.ts_init, UnixNanos::from(1));
    }
}
