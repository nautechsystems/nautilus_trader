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
use nautilus_execution::reports::fill::FillReport;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    types::{Money, Price, Quantity},
};

use super::messages::{FixMessage, fix_tag};
use crate::common::consts::COINBASE_INTX_VENUE;

/// Convert a FIX execution report to a Nautilus FillReport
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
        Ok(qty) => Quantity::new(qty, 8), // Use a reasonable default precision
        Err(e) => anyhow::bail!(format!("Invalid last quantity: {e}")),
    };

    let last_px = match last_px_str.parse::<f64>() {
        Ok(px) => Price::new(px, 8), // Use a reasonable default precision
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
