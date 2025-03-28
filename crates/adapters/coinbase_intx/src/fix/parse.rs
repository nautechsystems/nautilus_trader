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

use chrono::{DateTime, Utc};
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_execution::reports::{fill::FillReport, order::OrderStatusReport};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};
use ustr::Ustr;

use super::messages::{FixMessage, fix_tag};
use crate::common::{consts::COINBASE_INTX_VENUE, parse::parse_instrument_id};

// Reasonable default precision for now, as reports will be converted in the clients.
const DEFAULT_PRECISION: u8 = 8;

/// Parse a FIX execution report message to create a Nautilus OrderStatusReport.
pub(crate) fn convert_to_order_status_report(
    message: &FixMessage,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let venue_order_id = VenueOrderId::new(message.get_field_checked(fix_tag::ORDER_ID)?);
    let client_order_id = message
        .get_field(fix_tag::CL_ORD_ID)
        .map(ClientOrderId::new); // Can be missing

    let symbol = message.get_field_checked(fix_tag::SYMBOL)?;
    let instrument_id = parse_instrument_id(Ustr::from(symbol));

    let side = message.get_field_checked(fix_tag::SIDE)?;
    let order_side = match side {
        "1" => OrderSide::Buy,
        "2" => OrderSide::Sell,
        _ => return Err(anyhow::anyhow!("Unknown order side: {side}")),
    };

    let ord_type = message.get_field_checked(fix_tag::ORD_TYPE)?;
    let order_type = match ord_type {
        "1" => OrderType::Market,
        "2" => OrderType::Limit,
        "3" => OrderType::StopLimit,
        "4" => OrderType::StopMarket,
        _ => return Err(anyhow::anyhow!("Unknown order type: {ord_type}")),
    };

    let tif = message.get_field_checked(fix_tag::TIME_IN_FORCE)?;
    let time_in_force = match tif {
        "1" => TimeInForce::Gtc, // Good Till Cancel
        "3" => TimeInForce::Ioc, // Immediate or Cancel
        "4" => TimeInForce::Fok, // Fill or Kill
        "6" => TimeInForce::Gtd, // Good Till Date
        _ => return Err(anyhow::anyhow!("Unknown time in force: {tif}")),
    };

    let status = message.get_field_checked(fix_tag::ORD_STATUS)?;
    let order_status = match status {
        "0" => OrderStatus::Accepted, // New
        "1" => OrderStatus::PartiallyFilled,
        "2" => OrderStatus::Filled,
        "4" => OrderStatus::Canceled,
        "5" => OrderStatus::Rejected,
        "6" => OrderStatus::PendingCancel,
        "8" => OrderStatus::Rejected,
        "A" => OrderStatus::Submitted,     // Pending New
        "E" => OrderStatus::PendingUpdate, // Pending Replace
        "C" => OrderStatus::Expired,
        _ => return Err(anyhow::anyhow!("Unknown order status: {status}")),
    };

    let order_qty = message.get_field_checked(fix_tag::ORDER_QTY)?;
    let quantity = Quantity::new(order_qty.parse::<f64>()?, DEFAULT_PRECISION);

    let _leaves_qty = message.get_field_checked(fix_tag::LEAVES_QTY)?;
    let cum_qty = message.get_field_checked(fix_tag::CUM_QTY)?;
    let filled_qty = Quantity::new(cum_qty.parse::<f64>()?, DEFAULT_PRECISION);

    // Use TransactTime as the event time if provided
    let ts_last = if let Some(transact_time) = message.get_field(fix_tag::TRANSACT_TIME) {
        parse_fix_timestamp(transact_time).unwrap_or(ts_init)
    } else {
        ts_init
    };

    // For ts_accepted, we can only estimate based on available data
    // In practice, this might be tracked in your order management system
    let ts_accepted = ts_last;

    // Create the basic report
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
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
        None, // Report ID will be generated
    );

    if let Some(price_str) = message.get_field(fix_tag::PRICE) {
        if let Ok(price_val) = price_str.parse::<f64>() {
            report = report.with_price(Price::new(price_val, DEFAULT_PRECISION));
        }
    }

    if let Some(stop_px) = message.get_field(fix_tag::STOP_PX) {
        if let Ok(stop_val) = stop_px.parse::<f64>() {
            report = report.with_trigger_price(Price::new(stop_val, DEFAULT_PRECISION));
            report = report.with_trigger_type(TriggerType::LastPrice);
        }
    }

    if let Some(avg_px) = message.get_field(fix_tag::AVG_PX) {
        if let Ok(avg_val) = avg_px.parse::<f64>() {
            if avg_val > 0.0 {
                report = report.with_avg_px(avg_val);
            }
        }
    }

    // Execution instructions
    if let Some(exec_inst) = message.get_field(fix_tag::EXEC_INST) {
        // Parse space-delimited flags
        let flags: Vec<&str> = exec_inst.split(' ').collect();
        for flag in flags {
            match flag {
                "6" => report = report.with_post_only(true), // Post only
                "E" => report = report.with_reduce_only(true), // Close only
                _ => {}                                      // Ignore other flags
            }
        }
    }

    if let Some(expire_time) = message.get_field(fix_tag::EXPIRE_TIME) {
        if let Ok(dt) = parse_fix_timestamp(expire_time) {
            report = report.with_expire_time(dt);
        }
    }

    if let Some(text) = message.get_field(fix_tag::TEXT) {
        if !text.is_empty() {
            report = report.with_cancel_reason(text.to_string());
        }
    }

    Ok(report)
}

/// Parse a FIX execution report to a Nautilus FillReport
pub(crate) fn convert_to_fill_report(
    message: &FixMessage,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let client_order_id = message.get_field_checked(fix_tag::CL_ORD_ID)?;
    let venue_order_id = message.get_field_checked(fix_tag::ORDER_ID)?;
    let trade_id = message.get_field_checked(fix_tag::TRD_MATCH_ID)?;
    let symbol = message.get_field_checked(fix_tag::SYMBOL)?;
    let side_str = message.get_field_checked(fix_tag::SIDE)?;
    let last_qty_str = message.get_field_checked(fix_tag::LAST_QTY)?;
    let last_px_str = message.get_field_checked(fix_tag::LAST_PX)?;
    let currency = message.get_field_checked(fix_tag::CURRENCY)?.parse()?;
    let liquidity_indicator = message.get_field(fix_tag::LAST_LIQUIDITY_IND);

    let mut commission = Money::new(0.0, currency);

    if let Some(num_fees) = message.get_field(fix_tag::NO_MISC_FEES) {
        if let Ok(n) = num_fees.parse::<usize>() {
            // For simplicity, we'll just use the first fee
            if n > 0 {
                if let (Some(fee_amt), Some(fee_curr)) = (
                    message.get_field(fix_tag::MISC_FEE_AMT),
                    message.get_field(fix_tag::MISC_FEE_CURR),
                ) {
                    if let Ok(amt) = fee_amt.parse::<f64>() {
                        commission = Money::new(amt, fee_curr.parse().unwrap_or(currency));
                    }
                }
            }
        }
    }

    let client_order_id = ClientOrderId::new(client_order_id);
    let venue_order_id = VenueOrderId::new(venue_order_id);
    let trade_id = TradeId::new(trade_id);

    let order_side = match side_str {
        "1" => OrderSide::Buy,
        "2" => OrderSide::Sell,
        _ => anyhow::bail!("Unknown order side: {side_str}"),
    };

    let last_qty = match last_qty_str.parse::<f64>() {
        Ok(qty) => Quantity::new(qty, DEFAULT_PRECISION),
        Err(e) => anyhow::bail!(format!("Invalid last quantity: {e}")),
    };

    let last_px = match last_px_str.parse::<f64>() {
        Ok(px) => Price::new(px, DEFAULT_PRECISION),
        Err(e) => anyhow::bail!(format!("Invalid last price: {e}")),
    };

    let liquidity_side = match liquidity_indicator {
        Some("1") => LiquiditySide::Maker,
        Some("2") => LiquiditySide::Taker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    // Parse transaction time if available
    let ts_event = if let Some(transact_time) = message.get_field(fix_tag::TRANSACT_TIME) {
        if let Ok(dt) = DateTime::parse_from_str(transact_time, "%Y%m%d-%H:%M:%S%.3f") {
            UnixNanos::from(dt.with_timezone(&Utc))
        } else {
            ts_init
        }
    } else {
        ts_init
    };

    let instrument_id = InstrumentId::new(Symbol::from_str_unchecked(symbol), *COINBASE_INTX_VENUE);

    let report = FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        Some(client_order_id),
        None, // Position ID not applicable
        ts_event,
        get_atomic_clock_realtime().get_time_ns(),
        None, // UUID will be generated
    );

    Ok(report)
}

/// Parse a FIX timestamp in format YYYYMMDDd-HH:MM:SS.sss
fn parse_fix_timestamp(timestamp: &str) -> Result<UnixNanos, anyhow::Error> {
    let dt = DateTime::parse_from_str(timestamp, "%Y%m%d-%H:%M:%S%.3f")?;
    Ok(UnixNanos::from(dt.with_timezone(&Utc)))
}
