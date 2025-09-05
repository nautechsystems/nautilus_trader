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

use std::str::FromStr;

use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime, uuid::UUID4};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    data::TradeTick,
    enums::{CurrencyType, OrderSide, TriggerType},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, Symbol, TradeId, VenueOrderId,
    },
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;
use uuid::Uuid;

use super::models::{BitmexExecution, BitmexInstrument, BitmexOrder, BitmexPosition, BitmexTrade};
use crate::common::{
    enums::{BitmexExecInstruction, BitmexExecType, BitmexInstrumentType},
    parse::{
        map_bitmex_currency, parse_aggressor_side, parse_contingency_type, parse_instrument_id,
        parse_liquidity_side, parse_optional_datetime_to_unix_nanos, parse_order_status,
        parse_order_type, parse_position_side, parse_time_in_force,
    },
};

#[must_use]
pub fn parse_instrument_any(
    instrument: &BitmexInstrument,
    ts_init: UnixNanos,
) -> Option<InstrumentAny> {
    match instrument.instrument_type {
        BitmexInstrumentType::Spot => parse_spot_instrument(instrument, ts_init)
            .map_err(|e| {
                tracing::warn!("Failed to parse spot instrument {}: {e}", instrument.symbol);
                e
            })
            .ok(),
        BitmexInstrumentType::PerpetualContract | BitmexInstrumentType::PerpetualContractFx => {
            // Handle both crypto and FX perpetuals the same way
            parse_perpetual_instrument(instrument, ts_init)
                .map_err(|e| {
                    tracing::warn!(
                        "Failed to parse perpetual instrument {}: {e}",
                        instrument.symbol,
                    );
                    e
                })
                .ok()
        }
        BitmexInstrumentType::Futures => parse_futures_instrument(instrument, ts_init)
            .map_err(|e| {
                tracing::warn!(
                    "Failed to parse futures instrument {}: {e}",
                    instrument.symbol,
                );
                e
            })
            .ok(),
        BitmexInstrumentType::BasketIndex
        | BitmexInstrumentType::CryptoIndex
        | BitmexInstrumentType::FxIndex
        | BitmexInstrumentType::LendingIndex
        | BitmexInstrumentType::VolatilityIndex => {
            // Parse index instruments as perpetuals for cache purposes
            // They need to be in cache for WebSocket price updates
            parse_index_instrument(instrument, ts_init)
                .map_err(|e| {
                    tracing::warn!(
                        "Failed to parse index instrument {}: {}",
                        instrument.symbol,
                        e
                    );
                    e
                })
                .ok()
        }
        _ => {
            tracing::warn!(
                "Unsupported instrument type {:?} for symbol {}",
                instrument.instrument_type,
                instrument.symbol
            );
            None
        }
    }
}

/// Parse a BitMEX index instrument into a Nautilus `InstrumentAny`.
///
/// Index instruments are parsed as perpetuals with minimal fields to support
/// price update lookups in the WebSocket.
///
/// # Errors
///
/// Returns an error if values are out of valid range or cannot be parsed.
pub fn parse_index_instrument(
    definition: &BitmexInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::new(definition.symbol);

    // Use USD for indices
    let base_currency = Currency::USD();
    let quote_currency = Currency::USD();
    let settlement_currency = Currency::USD();

    let price_increment = Price::from(definition.tick_size.to_string());
    let size_increment = Quantity::from(1); // Indices don't have tradeable sizes

    Ok(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
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
        ts_init,
        ts_init,
    )))
}

/// Parse a BitMEX spot instrument into a Nautilus `InstrumentAny`.
///
/// # Errors
///
/// Returns an error if values are out of valid range or cannot be parsed.
pub fn parse_spot_instrument(
    definition: &BitmexInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::new(definition.symbol);
    let base_currency = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());

    let price_increment = Price::from(definition.tick_size.to_string());

    // For spot instruments, calculate the actual lot size using underlyingToPositionMultiplier
    // BitMEX spot uses lot_size / underlyingToPositionMultiplier for the actual minimum size
    let actual_lot_size = if let Some(multiplier) = definition.underlying_to_position_multiplier {
        definition.lot_size.unwrap_or(1.0) / multiplier
    } else {
        definition.lot_size.unwrap_or(1.0)
    };

    let size_increment = Quantity::from(actual_lot_size.to_string());

    let taker_fee = definition
        .taker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let maker_fee = definition
        .maker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    let lot_size = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_quantity = definition
        .max_order_qty
        .map(|qty| Quantity::new_checked(qty, 0))
        .transpose()?;
    let min_quantity = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = definition
        .max_price
        .map(|price| Price::from(price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // multiplier
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parse a BitMEX perpetual instrument into a Nautilus `InstrumentAny`.
///
/// # Errors
///
/// Returns an error if values are out of valid range or cannot be parsed.
pub fn parse_perpetual_instrument(
    definition: &BitmexInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::new(definition.symbol);
    let base_currency = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());
    let settlement_currency = get_currency(definition.settl_currency.as_ref().map_or_else(
        || definition.quote_currency.to_uppercase(),
        |s| s.to_uppercase(),
    ));
    let is_inverse = definition.is_inverse;

    let price_increment = Price::from(definition.tick_size.to_string());

    // For perpetual instruments, lot_size is typically already correct (usually 1.0)
    // But we should still check underlyingToPositionMultiplier for consistency
    let actual_lot_size = if let Some(multiplier) = definition.underlying_to_position_multiplier {
        definition.lot_size.unwrap_or(1.0) / multiplier
    } else {
        definition.lot_size.unwrap_or(1.0)
    };

    let size_increment = Quantity::from(actual_lot_size.to_string());

    let taker_fee = definition
        .taker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let maker_fee = definition
        .maker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    // TODO: How to handle negative multipliers?
    let multiplier = Some(Quantity::new_checked(definition.multiplier.abs(), 0)?);
    let lot_size = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_quantity = definition
        .max_order_qty
        .map(|qty| Quantity::new_checked(qty, 0))
        .transpose()?;
    let min_quantity = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = definition
        .max_price
        .map(|price| Price::from(price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parse a BitMEX futures instrument into a Nautilus `InstrumentAny`.
///
/// # Errors
///
/// Returns an error if values are out of valid range or cannot be parsed.
pub fn parse_futures_instrument(
    definition: &BitmexInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::new(definition.symbol);
    let underlying = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());
    let settlement_currency = get_currency(definition.settl_currency.as_ref().map_or_else(
        || definition.quote_currency.to_uppercase(),
        |s| s.to_uppercase(),
    ));
    let is_inverse = definition.is_inverse;

    let activation_ns = UnixNanos::from(definition.listing);
    let expiration_ns = parse_optional_datetime_to_unix_nanos(&definition.expiry, "expiry");
    let price_increment = Price::from(definition.tick_size.to_string());

    // For futures instruments, lot_size is typically already correct (usually 1.0)
    // But we should still check underlyingToPositionMultiplier for consistency
    let actual_lot_size = if let Some(multiplier) = definition.underlying_to_position_multiplier {
        definition.lot_size.unwrap_or(1.0) / multiplier
    } else {
        definition.lot_size.unwrap_or(1.0)
    };

    let size_increment = Quantity::from(actual_lot_size.to_string());

    let taker_fee = definition
        .taker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let maker_fee = definition
        .maker_fee
        .and_then(|fee| Decimal::from_str(&fee.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    // TODO: How to handle negative multipliers?
    let multiplier = Some(Quantity::new_checked(definition.multiplier.abs(), 0)?);

    let lot_size = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_quantity = definition
        .max_order_qty
        .map(|qty| Quantity::new_checked(qty, 0))
        .transpose()?;
    let min_quantity = definition
        .lot_size
        .map(|size| Quantity::new_checked(size, 0))
        .transpose()?;
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = definition
        .max_price
        .map(|price| Price::from(price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        is_inverse,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoFuture(instrument))
}

/// Parse a BitMEX trade into a Nautilus `TradeTick`.
///
/// # Errors
///
/// Currently this function does not return errors as all fields are handled gracefully,
/// but returns `Result` for future error handling compatibility.
pub fn parse_trade(
    trade: BitmexTrade,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = parse_instrument_id(trade.symbol);
    let price = Price::new(trade.price, price_precision);
    let size = Quantity::from(trade.size);
    let aggressor_side = parse_aggressor_side(&trade.side);
    let trade_id = TradeId::new(
        trade
            .trd_match_id
            .map_or_else(|| Uuid::new_v4().to_string(), |uuid| uuid.to_string()),
    );
    let ts_event = UnixNanos::from(trade.timestamp);

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

/// Parse a BitMEX order into a Nautilus `OrderStatusReport`.
///
/// # Errors
///
/// Currently this function does not return errors as all fields are handled gracefully,
/// but returns `Result` for future error handling compatibility.
///
/// # Panics
///
/// Panics if:
/// - Order is missing required fields: `symbol`, `ord_type`, `time_in_force`, `ord_status`, or `order_qty`
/// - Unsupported `ExecInstruction` type is encountered (other than `ParticipateDoNotInitiate` or `ReduceOnly`)
pub fn parse_order_status_report(
    order: &BitmexOrder,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let account_id = AccountId::new(format!("BITMEX-{}", order.account));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());
    let order_side: OrderSide = order
        .side
        .ok_or_else(|| anyhow::anyhow!("Order missing side"))?
        .into();
    let order_type = parse_order_type(
        order
            .ord_type
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Order missing ord_type"))?,
    );
    let time_in_force = parse_time_in_force(
        order
            .time_in_force
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Order missing time_in_force"))?,
    );
    let order_status = parse_order_status(
        order
            .ord_status
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Order missing ord_status"))?,
    );
    let quantity = Quantity::from(
        order
            .order_qty
            .ok_or_else(|| anyhow::anyhow!("Order missing order_qty"))?,
    );
    let filled_qty = Quantity::from(order.cum_qty.unwrap_or(0));
    let report_id = UUID4::new();
    let ts_accepted = order.transact_time.map_or_else(
        || get_atomic_clock_realtime().get_time_ns(),
        UnixNanos::from,
    );
    let ts_last = order.timestamp.map_or_else(
        || get_atomic_clock_realtime().get_time_ns(),
        UnixNanos::from,
    );

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

    if let Some(cl_ord_id) = order.cl_ord_id {
        report = report.with_client_order_id(ClientOrderId::new(cl_ord_id));
    }

    if let Some(cl_ord_link_id) = order.cl_ord_link_id {
        report = report.with_order_list_id(OrderListId::new(cl_ord_link_id));
    }

    if let Some(price) = order.price {
        report = report.with_price(Price::new(price, price_precision));
    }

    if let Some(avg_px) = order.avg_px {
        report = report.with_avg_px(avg_px);
    }

    if let Some(trigger_price) = order.stop_px {
        report = report
            .with_trigger_price(Price::new(trigger_price, price_precision))
            .with_trigger_type(TriggerType::Default);
    }

    if let Some(exec_instructions) = &order.exec_inst {
        for inst in exec_instructions {
            match inst {
                BitmexExecInstruction::ParticipateDoNotInitiate => {
                    report = report.with_post_only(true);
                }
                BitmexExecInstruction::ReduceOnly => report = report.with_reduce_only(true),
                BitmexExecInstruction::LastPrice
                | BitmexExecInstruction::Close
                | BitmexExecInstruction::MarkPrice
                | BitmexExecInstruction::IndexPrice
                | BitmexExecInstruction::AllOrNone
                | BitmexExecInstruction::Fixed
                | BitmexExecInstruction::Unknown => {}
            }
        }
    }

    if let Some(contingency_type) = order.contingency_type {
        report = report.with_contingency_type(parse_contingency_type(&contingency_type));
    }

    // if let Some(expire_time) = order.ex {
    //     report = report.with_trigger_price(Price::new(trigger_price, price_precision));
    // }

    Ok(report)
}

/// Parse a BitMEX execution into a Nautilus `FillReport`.
///
/// # Errors
///
/// Currently this function does not return errors as all fields are handled gracefully,
/// but returns `Result` for future error handling compatibility.
///
/// # Panics
///
/// Panics if:
/// - Execution is missing required fields: `symbol`, `order_id`, `trd_match_id`, `last_qty`, `last_px`, or `transact_time`
pub fn parse_fill_report(
    exec: BitmexExecution,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    // Skip non-trade executions (funding, settlements, etc.)
    // Trade executions have exec_type of Trade and must have order_id
    if !matches!(exec.exec_type, BitmexExecType::Trade) {
        return Err(anyhow::anyhow!(
            "Skipping non-trade execution: {:?}",
            exec.exec_type
        ));
    }

    // Additional check: skip executions without order_id (likely funding/settlement)
    if exec.order_id.is_none() {
        return Err(anyhow::anyhow!(
            "Skipping execution without order_id: {:?}",
            exec.exec_type
        ));
    }

    let account_id = AccountId::new(format!("BITMEX-{}", exec.account));
    let symbol = exec
        .symbol
        .ok_or_else(|| anyhow::anyhow!("Execution missing symbol"))?;
    let instrument_id = parse_instrument_id(symbol);
    // SAFETY: We already checked order_id is Some above
    let venue_order_id = VenueOrderId::new(exec.order_id.unwrap().to_string());
    // trd_match_id might be missing for some execution types, use exec_id as fallback
    let trade_id = TradeId::new(
        exec.trd_match_id
            .or(Some(exec.exec_id))
            .ok_or_else(|| anyhow::anyhow!("Fill missing both trd_match_id and exec_id"))?
            .to_string(),
    );
    // Skip executions without side (likely not trades)
    let Some(side) = exec.side else {
        return Err(anyhow::anyhow!(
            "Skipping execution without side: {:?}",
            exec.exec_type
        ));
    };
    let order_side: OrderSide = side.into();
    let last_qty = Quantity::from(exec.last_qty);
    let last_px = Price::new(exec.last_px, price_precision);

    // Map BitMEX currency to standard currency code
    let settlement_currency_str = exec.settl_currency.unwrap_or(Ustr::from("XBT")).as_str();
    let mapped_currency = map_bitmex_currency(settlement_currency_str);
    let commission = Money::new(
        exec.commission.unwrap_or(0.0),
        Currency::from(mapped_currency.as_str()),
    );
    let liquidity_side = parse_liquidity_side(&exec.last_liquidity_ind);
    let client_order_id = exec.cl_ord_id.map(ClientOrderId::new);
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_event = exec.transact_time.map_or_else(
        || get_atomic_clock_realtime().get_time_ns(),
        UnixNanos::from,
    );

    Ok(FillReport::new(
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

/// Parse a BitMEX position into a Nautilus `PositionStatusReport`.
///
/// # Errors
///
/// Currently this function does not return errors as all fields are handled gracefully,
/// but returns `Result` for future error handling compatibility.
pub fn parse_position_report(
    position: BitmexPosition,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let account_id = AccountId::new(format!("BITMEX-{}", position.account));
    let instrument_id = parse_instrument_id(position.symbol);
    let position_side = parse_position_side(position.current_qty).as_specified();
    let quantity = Quantity::from(position.current_qty.map_or(0_i64, i64::abs));
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_last = parse_optional_datetime_to_unix_nanos(&position.timestamp, "timestamp");

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        venue_position_id,
        ts_last,
        ts_init,
        None,
    ))
}

/// Returns the currency either from the internal currency map or creates a default crypto.
fn get_currency(code: String) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(&code)
        .copied()
        .unwrap_or(Currency::new(&code, 8, 0, &code, CurrencyType::Crypto))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use nautilus_model::enums::{LiquiditySide, PositionSide};
    use rstest::rstest;
    use rust_decimal::prelude::ToPrimitive;
    use uuid::Uuid;

    use super::*;
    use crate::{
        common::{
            enums::{
                BitmexContingencyType, BitmexFairMethod, BitmexInstrumentState,
                BitmexInstrumentType, BitmexLiquidityIndicator, BitmexMarkMethod,
                BitmexOrderStatus, BitmexOrderType, BitmexSide, BitmexTickDirection,
                BitmexTimeInForce,
            },
            testing::load_test_json,
        },
        http::models::{
            BitmexExecution, BitmexInstrument, BitmexOrder, BitmexPosition, BitmexTradeBin,
            BitmexWallet,
        },
    };

    #[rstest]
    fn test_perp_instrument_deserialization() {
        let json_data = load_test_json("http_get_instrument_xbtusd.json");
        let instrument: BitmexInstrument = serde_json::from_str(&json_data).unwrap();

        assert_eq!(instrument.symbol, "XBTUSD");
        assert_eq!(instrument.root_symbol, "XBT");
        assert_eq!(instrument.state, BitmexInstrumentState::Open);
        assert!(instrument.is_inverse);
        assert_eq!(instrument.maker_fee, Some(0.0005));
        assert_eq!(
            instrument.timestamp.to_rfc3339(),
            "2024-11-24T23:33:19.034+00:00"
        );
    }

    #[rstest]
    fn test_parse_orders() {
        let json_data = load_test_json("http_get_orders.json");
        let orders: Vec<BitmexOrder> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(orders.len(), 2);

        // Test first order (New)
        let order1 = &orders[0];
        assert_eq!(order1.symbol, Some(Ustr::from("XBTUSD")));
        assert_eq!(order1.side, Some(BitmexSide::Buy));
        assert_eq!(order1.order_qty, Some(100));
        assert_eq!(order1.price, Some(98000.0));
        assert_eq!(order1.ord_status, Some(BitmexOrderStatus::New));
        assert_eq!(order1.leaves_qty, Some(100));
        assert_eq!(order1.cum_qty, Some(0));

        // Test second order (Filled)
        let order2 = &orders[1];
        assert_eq!(order2.symbol, Some(Ustr::from("XBTUSD")));
        assert_eq!(order2.side, Some(BitmexSide::Sell));
        assert_eq!(order2.order_qty, Some(200));
        assert_eq!(order2.ord_status, Some(BitmexOrderStatus::Filled));
        assert_eq!(order2.leaves_qty, Some(0));
        assert_eq!(order2.cum_qty, Some(200));
        assert_eq!(order2.avg_px, Some(98950.5));
    }

    #[rstest]
    fn test_parse_executions() {
        let json_data = load_test_json("http_get_executions.json");
        let executions: Vec<BitmexExecution> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(executions.len(), 2);

        // Test first execution (Maker)
        let exec1 = &executions[0];
        assert_eq!(exec1.symbol, Some(Ustr::from("XBTUSD")));
        assert_eq!(exec1.side, Some(BitmexSide::Sell));
        assert_eq!(exec1.last_qty, 100);
        assert_eq!(exec1.last_px, 98950.0);
        assert_eq!(
            exec1.last_liquidity_ind,
            Some(BitmexLiquidityIndicator::Maker)
        );
        assert_eq!(exec1.commission, Some(0.00075));

        // Test second execution (Taker)
        let exec2 = &executions[1];
        assert_eq!(
            exec2.last_liquidity_ind,
            Some(BitmexLiquidityIndicator::Taker)
        );
        assert_eq!(exec2.last_px, 98951.0);
    }

    #[rstest]
    fn test_parse_positions() {
        let json_data = load_test_json("http_get_positions.json");
        let positions: Vec<BitmexPosition> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(positions.len(), 1);

        let position = &positions[0];
        assert_eq!(position.account, 1234567);
        assert_eq!(position.symbol, "XBTUSD");
        assert_eq!(position.current_qty, Some(100));
        assert_eq!(position.avg_entry_price, Some(98390.88));
        assert_eq!(position.unrealised_pnl, Some(1350));
        assert_eq!(position.realised_pnl, Some(-227));
        assert_eq!(position.is_open, Some(true));
    }

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("http_get_trades.json");
        let trades: Vec<BitmexTrade> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(trades.len(), 3);

        // Test first trade
        let trade1 = &trades[0];
        assert_eq!(trade1.symbol, "XBTUSD");
        assert_eq!(trade1.side, Some(BitmexSide::Buy));
        assert_eq!(trade1.size, 100);
        assert_eq!(trade1.price, 98950.0);

        // Test third trade (Sell side)
        let trade3 = &trades[2];
        assert_eq!(trade3.side, Some(BitmexSide::Sell));
        assert_eq!(trade3.size, 50);
        assert_eq!(trade3.price, 98949.5);
    }

    #[rstest]
    fn test_parse_wallet() {
        let json_data = load_test_json("http_get_wallet.json");
        let wallets: Vec<BitmexWallet> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(wallets.len(), 1);

        let wallet = &wallets[0];
        assert_eq!(wallet.account, 1234567);
        assert_eq!(wallet.currency, "XBt");
        assert_eq!(wallet.amount, Some(1000123456));
        assert_eq!(wallet.delta_amount, Some(123456));
    }

    #[rstest]
    fn test_parse_trade_bins() {
        let json_data = load_test_json("http_get_trade_bins.json");
        let bins: Vec<BitmexTradeBin> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(bins.len(), 3);

        // Test first bin
        let bin1 = &bins[0];
        assert_eq!(bin1.symbol, "XBTUSD");
        assert_eq!(bin1.open, Some(98900.0));
        assert_eq!(bin1.high, Some(98980.5));
        assert_eq!(bin1.low, Some(98890.0));
        assert_eq!(bin1.close, Some(98950.0));
        assert_eq!(bin1.volume, Some(150000));
        assert_eq!(bin1.trades, Some(45));

        // Test last bin
        let bin3 = &bins[2];
        assert_eq!(bin3.close, Some(98970.0));
        assert_eq!(bin3.volume, Some(78000));
    }

    #[rstest]
    fn test_parse_order_status_report() {
        let symbol = Ustr::from("XBTUSD");

        let order = BitmexOrder {
            account: 123456,
            symbol: Some(Ustr::from("XBTUSD")),
            order_id: Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap(),
            cl_ord_id: Some(Ustr::from("client-123")),
            cl_ord_link_id: None,
            side: Some(BitmexSide::Buy),
            ord_type: Some(BitmexOrderType::Limit),
            time_in_force: Some(BitmexTimeInForce::GoodTillCancel),
            ord_status: Some(BitmexOrderStatus::New),
            order_qty: Some(100),
            cum_qty: Some(50),
            price: Some(50000.0),
            stop_px: Some(49000.0),
            display_qty: None,
            peg_offset_value: None,
            peg_price_type: None,
            currency: Some(Ustr::from("USD")),
            settl_currency: Some(Ustr::from("XBt")),
            exec_inst: Some(vec![
                BitmexExecInstruction::ParticipateDoNotInitiate,
                BitmexExecInstruction::ReduceOnly,
            ]),
            contingency_type: Some(BitmexContingencyType::OneCancelsTheOther),
            ex_destination: None,
            triggered: None,
            working_indicator: Some(true),
            ord_rej_reason: None,
            leaves_qty: Some(50),
            avg_px: None,
            multi_leg_reporting_type: None,
            text: None,
            transact_time: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:01Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        };

        let instrument_id = parse_instrument_id(symbol);
        let report =
            parse_order_status_report(&order, instrument_id, 2, UnixNanos::from(1)).unwrap();

        assert_eq!(report.account_id.to_string(), "BITMEX-123456");
        assert_eq!(report.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(
            report.venue_order_id.as_str(),
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        );
        assert_eq!(report.client_order_id.unwrap().as_str(), "client-123");
        assert_eq!(report.quantity.as_f64(), 100.0);
        assert_eq!(report.filled_qty.as_f64(), 50.0);
        assert_eq!(report.price.unwrap().as_f64(), 50000.0);
        assert_eq!(report.trigger_price.unwrap().as_f64(), 49000.0);
        assert!(report.post_only);
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_parse_order_status_report_minimal() {
        let symbol = Ustr::from("ETHUSD");
        let order = BitmexOrder {
            account: 0, // Use 0 for test account
            symbol: Some(Ustr::from("ETHUSD")),
            order_id: Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
            cl_ord_id: None,
            cl_ord_link_id: None,
            side: Some(BitmexSide::Sell),
            ord_type: Some(BitmexOrderType::Market),
            time_in_force: Some(BitmexTimeInForce::ImmediateOrCancel),
            ord_status: Some(BitmexOrderStatus::Filled),
            order_qty: Some(200),
            cum_qty: Some(200),
            price: None,
            stop_px: None,
            display_qty: None,
            peg_offset_value: None,
            peg_price_type: None,
            currency: None,
            settl_currency: None,
            exec_inst: None,
            contingency_type: None,
            ex_destination: None,
            triggered: None,
            working_indicator: Some(false),
            ord_rej_reason: None,
            leaves_qty: Some(0),
            avg_px: None,
            multi_leg_reporting_type: None,
            text: None,
            transact_time: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:01Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        };

        let instrument_id = parse_instrument_id(symbol);
        let report =
            parse_order_status_report(&order, instrument_id, 2, UnixNanos::from(1)).unwrap();

        assert_eq!(report.account_id.to_string(), "BITMEX-0");
        assert_eq!(report.instrument_id.to_string(), "ETHUSD.BITMEX");
        assert_eq!(
            report.venue_order_id.as_str(),
            "11111111-2222-3333-4444-555555555555"
        );
        assert!(report.client_order_id.is_none());
        assert_eq!(report.quantity.as_f64(), 200.0);
        assert_eq!(report.filled_qty.as_f64(), 200.0);
        assert!(report.price.is_none());
        assert!(report.trigger_price.is_none());
        assert!(!report.post_only);
        assert!(!report.reduce_only);
    }

    #[rstest]
    fn test_parse_fill_report() {
        let exec = BitmexExecution {
            exec_id: Uuid::parse_str("f1f2f3f4-e5e6-d7d8-c9c0-b1b2b3b4b5b6").unwrap(),
            account: 654321,
            symbol: Some(Ustr::from("XBTUSD")),
            order_id: Some(Uuid::parse_str("a1a2a3a4-b5b6-c7c8-d9d0-e1e2e3e4e5e6").unwrap()),
            cl_ord_id: Some(Ustr::from("client-456")),
            side: Some(BitmexSide::Buy),
            last_qty: 50,
            last_px: 50100.5,
            commission: Some(0.00075),
            settl_currency: Some(Ustr::from("XBt")),
            last_liquidity_ind: Some(BitmexLiquidityIndicator::Taker),
            trd_match_id: Some(Uuid::parse_str("99999999-8888-7777-6666-555555555555").unwrap()),
            transact_time: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            cl_ord_link_id: None,
            underlying_last_px: None,
            last_mkt: None,
            order_qty: Some(50),
            price: Some(50100.0),
            display_qty: None,
            stop_px: None,
            peg_offset_value: None,
            peg_price_type: None,
            currency: None,
            exec_type: BitmexExecType::Trade,
            ord_type: BitmexOrderType::Limit,
            time_in_force: BitmexTimeInForce::GoodTillCancel,
            exec_inst: None,
            contingency_type: None,
            ex_destination: None,
            ord_status: Some(BitmexOrderStatus::Filled),
            triggered: None,
            working_indicator: None,
            ord_rej_reason: None,
            leaves_qty: None,
            cum_qty: Some(50),
            avg_px: Some(50100.5),
            trade_publish_indicator: None,
            multi_leg_reporting_type: None,
            text: None,
            exec_cost: None,
            exec_comm: None,
            home_notional: None,
            foreign_notional: None,
            timestamp: None,
        };

        let report = parse_fill_report(exec, 2, UnixNanos::from(1)).unwrap();

        assert_eq!(report.account_id.to_string(), "BITMEX-654321");
        assert_eq!(report.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(
            report.venue_order_id.as_str(),
            "a1a2a3a4-b5b6-c7c8-d9d0-e1e2e3e4e5e6"
        );
        assert_eq!(
            report.trade_id.to_string(),
            "99999999-8888-7777-6666-555555555555"
        );
        assert_eq!(report.client_order_id.unwrap().as_str(), "client-456");
        assert_eq!(report.last_qty.as_f64(), 50.0);
        assert_eq!(report.last_px.as_f64(), 50100.5);
        assert_eq!(report.commission.as_f64(), 0.00075);
        assert_eq!(report.commission.currency.code.as_str(), "XBT");
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
    }

    #[rstest]
    fn test_parse_fill_report_with_missing_trd_match_id() {
        let exec = BitmexExecution {
            exec_id: Uuid::parse_str("f1f2f3f4-e5e6-d7d8-c9c0-b1b2b3b4b5b6").unwrap(),
            account: 111111,
            symbol: Some(Ustr::from("ETHUSD")),
            order_id: Some(Uuid::parse_str("a1a2a3a4-b5b6-c7c8-d9d0-e1e2e3e4e5e6").unwrap()),
            cl_ord_id: None,
            side: Some(BitmexSide::Sell),
            last_qty: 100,
            last_px: 3000.0,
            commission: None,
            settl_currency: None,
            last_liquidity_ind: Some(BitmexLiquidityIndicator::Maker),
            trd_match_id: None, // Missing, should fall back to exec_id
            transact_time: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            cl_ord_link_id: None,
            underlying_last_px: None,
            last_mkt: None,
            order_qty: Some(100),
            price: Some(3000.0),
            display_qty: None,
            stop_px: None,
            peg_offset_value: None,
            peg_price_type: None,
            currency: None,
            exec_type: BitmexExecType::Trade,
            ord_type: BitmexOrderType::Market,
            time_in_force: BitmexTimeInForce::ImmediateOrCancel,
            exec_inst: None,
            contingency_type: None,
            ex_destination: None,
            ord_status: Some(BitmexOrderStatus::Filled),
            triggered: None,
            working_indicator: None,
            ord_rej_reason: None,
            leaves_qty: None,
            cum_qty: Some(100),
            avg_px: Some(3000.0),
            trade_publish_indicator: None,
            multi_leg_reporting_type: None,
            text: None,
            exec_cost: None,
            exec_comm: None,
            home_notional: None,
            foreign_notional: None,
            timestamp: None,
        };

        let report = parse_fill_report(exec, 2, UnixNanos::from(1)).unwrap();

        assert_eq!(report.account_id.to_string(), "BITMEX-111111");
        assert_eq!(
            report.trade_id.to_string(),
            "f1f2f3f4-e5e6-d7d8-c9c0-b1b2b3b4b5b6"
        );
        assert!(report.client_order_id.is_none());
        assert_eq!(report.commission.as_f64(), 0.0);
        assert_eq!(report.commission.currency.code.as_str(), "XBT");
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }

    #[rstest]
    fn test_parse_position_report() {
        let position = BitmexPosition {
            account: 789012,
            symbol: Ustr::from("XBTUSD"),
            current_qty: Some(1000),
            timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            currency: None,
            underlying: None,
            quote_currency: None,
            commission: None,
            init_margin_req: None,
            maint_margin_req: None,
            risk_limit: None,
            leverage: None,
            cross_margin: None,
            deleverage_percentile: None,
            rebalanced_pnl: None,
            prev_realised_pnl: None,
            prev_unrealised_pnl: None,
            prev_close_price: None,
            opening_timestamp: None,
            opening_qty: None,
            opening_cost: None,
            opening_comm: None,
            open_order_buy_qty: None,
            open_order_buy_cost: None,
            open_order_buy_premium: None,
            open_order_sell_qty: None,
            open_order_sell_cost: None,
            open_order_sell_premium: None,
            exec_buy_qty: None,
            exec_buy_cost: None,
            exec_sell_qty: None,
            exec_sell_cost: None,
            exec_qty: None,
            exec_cost: None,
            exec_comm: None,
            current_timestamp: None,
            current_cost: None,
            current_comm: None,
            realised_cost: None,
            unrealised_cost: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            is_open: Some(true),
            mark_price: None,
            mark_value: None,
            risk_value: None,
            home_notional: None,
            foreign_notional: None,
            pos_state: None,
            pos_cost: None,
            pos_cost2: None,
            pos_cross: None,
            pos_init: None,
            pos_comm: None,
            pos_loss: None,
            pos_margin: None,
            pos_maint: None,
            pos_allowance: None,
            taxable_margin: None,
            init_margin: None,
            maint_margin: None,
            session_margin: None,
            target_excess_margin: None,
            var_margin: None,
            realised_gross_pnl: None,
            realised_tax: None,
            realised_pnl: None,
            unrealised_gross_pnl: None,
            long_bankrupt: None,
            short_bankrupt: None,
            tax_base: None,
            indicative_tax_rate: None,
            indicative_tax: None,
            unrealised_tax: None,
            unrealised_pnl: None,
            unrealised_pnl_pcnt: None,
            unrealised_roe_pcnt: None,
            avg_cost_price: None,
            avg_entry_price: None,
            break_even_price: None,
            margin_call_price: None,
            liquidation_price: None,
            bankrupt_price: None,
            last_price: None,
            last_value: None,
        };

        let report = parse_position_report(position, UnixNanos::from(1)).unwrap();

        assert_eq!(report.account_id.to_string(), "BITMEX-789012");
        assert_eq!(report.instrument_id.to_string(), "XBTUSD.BITMEX");
        assert_eq!(report.position_side.as_position_side(), PositionSide::Long);
        assert_eq!(report.quantity.as_f64(), 1000.0);
    }

    #[rstest]
    fn test_parse_position_report_short() {
        let position = BitmexPosition {
            account: 789012,
            symbol: Ustr::from("ETHUSD"),
            current_qty: Some(-500),
            timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            currency: None,
            underlying: None,
            quote_currency: None,
            commission: None,
            init_margin_req: None,
            maint_margin_req: None,
            risk_limit: None,
            leverage: None,
            cross_margin: None,
            deleverage_percentile: None,
            rebalanced_pnl: None,
            prev_realised_pnl: None,
            prev_unrealised_pnl: None,
            prev_close_price: None,
            opening_timestamp: None,
            opening_qty: None,
            opening_cost: None,
            opening_comm: None,
            open_order_buy_qty: None,
            open_order_buy_cost: None,
            open_order_buy_premium: None,
            open_order_sell_qty: None,
            open_order_sell_cost: None,
            open_order_sell_premium: None,
            exec_buy_qty: None,
            exec_buy_cost: None,
            exec_sell_qty: None,
            exec_sell_cost: None,
            exec_qty: None,
            exec_cost: None,
            exec_comm: None,
            current_timestamp: None,
            current_cost: None,
            current_comm: None,
            realised_cost: None,
            unrealised_cost: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            is_open: Some(true),
            mark_price: None,
            mark_value: None,
            risk_value: None,
            home_notional: None,
            foreign_notional: None,
            pos_state: None,
            pos_cost: None,
            pos_cost2: None,
            pos_cross: None,
            pos_init: None,
            pos_comm: None,
            pos_loss: None,
            pos_margin: None,
            pos_maint: None,
            pos_allowance: None,
            taxable_margin: None,
            init_margin: None,
            maint_margin: None,
            session_margin: None,
            target_excess_margin: None,
            var_margin: None,
            realised_gross_pnl: None,
            realised_tax: None,
            realised_pnl: None,
            unrealised_gross_pnl: None,
            long_bankrupt: None,
            short_bankrupt: None,
            tax_base: None,
            indicative_tax_rate: None,
            indicative_tax: None,
            unrealised_tax: None,
            unrealised_pnl: None,
            unrealised_pnl_pcnt: None,
            unrealised_roe_pcnt: None,
            avg_cost_price: None,
            avg_entry_price: None,
            break_even_price: None,
            margin_call_price: None,
            liquidation_price: None,
            bankrupt_price: None,
            last_price: None,
            last_value: None,
        };

        let report = parse_position_report(position, UnixNanos::from(1)).unwrap();

        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity.as_f64(), 500.0); // Should be absolute value
    }

    #[rstest]
    fn test_parse_position_report_flat() {
        let position = BitmexPosition {
            account: 789012,
            symbol: Ustr::from("SOLUSD"),
            current_qty: Some(0),
            timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            currency: None,
            underlying: None,
            quote_currency: None,
            commission: None,
            init_margin_req: None,
            maint_margin_req: None,
            risk_limit: None,
            leverage: None,
            cross_margin: None,
            deleverage_percentile: None,
            rebalanced_pnl: None,
            prev_realised_pnl: None,
            prev_unrealised_pnl: None,
            prev_close_price: None,
            opening_timestamp: None,
            opening_qty: None,
            opening_cost: None,
            opening_comm: None,
            open_order_buy_qty: None,
            open_order_buy_cost: None,
            open_order_buy_premium: None,
            open_order_sell_qty: None,
            open_order_sell_cost: None,
            open_order_sell_premium: None,
            exec_buy_qty: None,
            exec_buy_cost: None,
            exec_sell_qty: None,
            exec_sell_cost: None,
            exec_qty: None,
            exec_cost: None,
            exec_comm: None,
            current_timestamp: None,
            current_cost: None,
            current_comm: None,
            realised_cost: None,
            unrealised_cost: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            is_open: Some(true),
            mark_price: None,
            mark_value: None,
            risk_value: None,
            home_notional: None,
            foreign_notional: None,
            pos_state: None,
            pos_cost: None,
            pos_cost2: None,
            pos_cross: None,
            pos_init: None,
            pos_comm: None,
            pos_loss: None,
            pos_margin: None,
            pos_maint: None,
            pos_allowance: None,
            taxable_margin: None,
            init_margin: None,
            maint_margin: None,
            session_margin: None,
            target_excess_margin: None,
            var_margin: None,
            realised_gross_pnl: None,
            realised_tax: None,
            realised_pnl: None,
            unrealised_gross_pnl: None,
            long_bankrupt: None,
            short_bankrupt: None,
            tax_base: None,
            indicative_tax_rate: None,
            indicative_tax: None,
            unrealised_tax: None,
            unrealised_pnl: None,
            unrealised_pnl_pcnt: None,
            unrealised_roe_pcnt: None,
            avg_cost_price: None,
            avg_entry_price: None,
            break_even_price: None,
            margin_call_price: None,
            liquidation_price: None,
            bankrupt_price: None,
            last_price: None,
            last_value: None,
        };

        let report = parse_position_report(position, UnixNanos::from(1)).unwrap();

        assert_eq!(report.position_side.as_position_side(), PositionSide::Flat);
        assert_eq!(report.quantity.as_f64(), 0.0);
    }

    // ========================================================================
    // Test Fixtures for Instrument Parsing
    // ========================================================================

    fn create_test_spot_instrument() -> BitmexInstrument {
        BitmexInstrument {
            symbol: Ustr::from("XBTUSD"),
            root_symbol: Ustr::from("XBT"),
            state: BitmexInstrumentState::Open,
            instrument_type: BitmexInstrumentType::Spot,
            listing: DateTime::parse_from_rfc3339("2016-05-13T12:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            front: Some(
                DateTime::parse_from_rfc3339("2016-05-13T12:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            expiry: None,
            settle: None,
            listed_settle: None,
            position_currency: Some(Ustr::from("USD")),
            underlying: Ustr::from("XBT"),
            quote_currency: Ustr::from("USD"),
            underlying_symbol: Some(Ustr::from("XBT=")),
            reference: Some(Ustr::from("BMEX")),
            reference_symbol: Some(Ustr::from(".BXBT")),
            lot_size: Some(1.0),
            tick_size: 0.01,
            multiplier: 1.0,
            settl_currency: Some(Ustr::from("USD")),
            is_quanto: false,
            is_inverse: false,
            maker_fee: Some(-0.00025),
            taker_fee: Some(0.00075),
            timestamp: DateTime::parse_from_rfc3339("2024-01-01T00:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            // Set other fields to reasonable defaults
            max_order_qty: Some(10000000.0),
            max_price: Some(1000000.0),
            settlement_fee: Some(0.0),
            mark_price: Some(50500.0),
            last_price: Some(50500.0),
            bid_price: Some(50499.5),
            ask_price: Some(50500.5),
            open_interest: Some(0.0),
            open_value: Some(0.0),
            total_volume: Some(1000000.0),
            volume: Some(50000.0),
            volume_24h: Some(75000.0),
            total_turnover: Some(150000000.0),
            turnover: Some(5000000.0),
            turnover_24h: Some(7500000.0),
            has_liquidity: Some(true),
            // Set remaining fields to None/defaults
            calc_interval: None,
            publish_interval: None,
            publish_time: None,
            underlying_to_position_multiplier: Some(1.0),
            underlying_to_settle_multiplier: None,
            quote_to_settle_multiplier: Some(1.0),
            init_margin: Some(0.1),
            maint_margin: Some(0.05),
            risk_limit: Some(20000000000.0),
            risk_step: Some(10000000000.0),
            limit: None,
            taxed: Some(true),
            deleverage: Some(true),
            funding_base_symbol: None,
            funding_quote_symbol: None,
            funding_premium_symbol: None,
            funding_timestamp: None,
            funding_interval: None,
            funding_rate: None,
            indicative_funding_rate: None,
            rebalance_timestamp: None,
            rebalance_interval: None,
            prev_close_price: Some(50000.0),
            limit_down_price: None,
            limit_up_price: None,
            prev_total_turnover: Some(100000000.0),
            home_notional_24h: Some(1.5),
            foreign_notional_24h: Some(75000.0),
            prev_price_24h: Some(49500.0),
            vwap: Some(50100.0),
            high_price: Some(51000.0),
            low_price: Some(49000.0),
            last_price_protected: Some(50500.0),
            last_tick_direction: Some(BitmexTickDirection::PlusTick),
            last_change_pcnt: Some(0.0202),
            mid_price: Some(50500.0),
            impact_bid_price: Some(50490.0),
            impact_mid_price: Some(50495.0),
            impact_ask_price: Some(50500.0),
            fair_method: None,
            fair_basis_rate: None,
            fair_basis: None,
            fair_price: None,
            mark_method: Some(BitmexMarkMethod::LastPrice),
            indicative_settle_price: None,
            settled_price_adjustment_rate: None,
            settled_price: None,
            instant_pnl: false,
            min_tick: None,
            funding_base_rate: None,
            funding_quote_rate: None,
        }
    }

    fn create_test_perpetual_instrument() -> BitmexInstrument {
        BitmexInstrument {
            symbol: Ustr::from("XBTUSD"),
            root_symbol: Ustr::from("XBT"),
            state: BitmexInstrumentState::Open,
            instrument_type: BitmexInstrumentType::PerpetualContract,
            listing: DateTime::parse_from_rfc3339("2016-05-13T12:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            front: Some(
                DateTime::parse_from_rfc3339("2016-05-13T12:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            expiry: None,
            settle: None,
            listed_settle: None,
            position_currency: Some(Ustr::from("USD")),
            underlying: Ustr::from("XBT"),
            quote_currency: Ustr::from("USD"),
            underlying_symbol: Some(Ustr::from("XBT=")),
            reference: Some(Ustr::from("BMEX")),
            reference_symbol: Some(Ustr::from(".BXBT")),
            lot_size: Some(1.0),
            tick_size: 0.5,
            multiplier: -1.0,
            settl_currency: Some(Ustr::from("XBT")),
            is_quanto: false,
            is_inverse: true,
            maker_fee: Some(-0.00025),
            taker_fee: Some(0.00075),
            timestamp: DateTime::parse_from_rfc3339("2024-01-01T00:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            // Set other fields
            max_order_qty: Some(10000000.0),
            max_price: Some(1000000.0),
            settlement_fee: Some(0.0),
            mark_price: Some(50500.01),
            last_price: Some(50500.0),
            bid_price: Some(50499.5),
            ask_price: Some(50500.5),
            open_interest: Some(500000000.0),
            open_value: Some(990099009900.0),
            total_volume: Some(12345678900000.0),
            volume: Some(5000000.0),
            volume_24h: Some(75000000.0),
            total_turnover: Some(150000000000000.0),
            turnover: Some(5000000000.0),
            turnover_24h: Some(7500000000.0),
            has_liquidity: Some(true),
            // Perpetual specific fields
            funding_base_symbol: Some(Ustr::from(".XBTBON8H")),
            funding_quote_symbol: Some(Ustr::from(".USDBON8H")),
            funding_premium_symbol: Some(Ustr::from(".XBTUSDPI8H")),
            funding_timestamp: Some(
                DateTime::parse_from_rfc3339("2024-01-01T08:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            funding_interval: Some(
                DateTime::parse_from_rfc3339("2000-01-01T08:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            funding_rate: Some(0.0001),
            indicative_funding_rate: Some(0.0001),
            funding_base_rate: Some(0.01),
            funding_quote_rate: Some(-0.01),
            // Other fields
            calc_interval: None,
            publish_interval: None,
            publish_time: None,
            underlying_to_position_multiplier: Some(1.0),
            underlying_to_settle_multiplier: None,
            quote_to_settle_multiplier: Some(0.00000001),
            init_margin: Some(0.01),
            maint_margin: Some(0.005),
            risk_limit: Some(20000000000.0),
            risk_step: Some(10000000000.0),
            limit: None,
            taxed: Some(true),
            deleverage: Some(true),
            rebalance_timestamp: None,
            rebalance_interval: None,
            prev_close_price: Some(50000.0),
            limit_down_price: None,
            limit_up_price: None,
            prev_total_turnover: Some(100000000000000.0),
            home_notional_24h: Some(1500.0),
            foreign_notional_24h: Some(75000000.0),
            prev_price_24h: Some(49500.0),
            vwap: Some(50100.0),
            high_price: Some(51000.0),
            low_price: Some(49000.0),
            last_price_protected: Some(50500.0),
            last_tick_direction: Some(BitmexTickDirection::PlusTick),
            last_change_pcnt: Some(0.0202),
            mid_price: Some(50500.0),
            impact_bid_price: Some(50490.0),
            impact_mid_price: Some(50495.0),
            impact_ask_price: Some(50500.0),
            fair_method: Some(BitmexFairMethod::FundingRate),
            fair_basis_rate: Some(0.1095),
            fair_basis: Some(0.01),
            fair_price: Some(50500.01),
            mark_method: Some(BitmexMarkMethod::FairPrice),
            indicative_settle_price: Some(50500.0),
            settled_price_adjustment_rate: None,
            settled_price: None,
            instant_pnl: false,
            min_tick: None,
        }
    }

    fn create_test_futures_instrument() -> BitmexInstrument {
        BitmexInstrument {
            symbol: Ustr::from("XBTH25"),
            root_symbol: Ustr::from("XBT"),
            state: BitmexInstrumentState::Open,
            instrument_type: BitmexInstrumentType::Futures,
            listing: DateTime::parse_from_rfc3339("2024-09-27T12:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            front: Some(
                DateTime::parse_from_rfc3339("2024-12-27T12:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            expiry: Some(
                DateTime::parse_from_rfc3339("2025-03-28T12:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            settle: Some(
                DateTime::parse_from_rfc3339("2025-03-28T12:00:00.000Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            listed_settle: None,
            position_currency: Some(Ustr::from("USD")),
            underlying: Ustr::from("XBT"),
            quote_currency: Ustr::from("USD"),
            underlying_symbol: Some(Ustr::from("XBT=")),
            reference: Some(Ustr::from("BMEX")),
            reference_symbol: Some(Ustr::from(".BXBT30M")),
            lot_size: Some(1.0),
            tick_size: 0.5,
            multiplier: -1.0,
            settl_currency: Some(Ustr::from("XBT")),
            is_quanto: false,
            is_inverse: true,
            maker_fee: Some(-0.00025),
            taker_fee: Some(0.00075),
            settlement_fee: Some(0.0005),
            timestamp: DateTime::parse_from_rfc3339("2024-01-01T00:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            // Set other fields
            max_order_qty: Some(10000000.0),
            max_price: Some(1000000.0),
            mark_price: Some(55500.0),
            last_price: Some(55500.0),
            bid_price: Some(55499.5),
            ask_price: Some(55500.5),
            open_interest: Some(50000000.0),
            open_value: Some(90090090090.0),
            total_volume: Some(1000000000.0),
            volume: Some(500000.0),
            volume_24h: Some(7500000.0),
            total_turnover: Some(15000000000000.0),
            turnover: Some(500000000.0),
            turnover_24h: Some(750000000.0),
            has_liquidity: Some(true),
            // Futures specific fields
            funding_base_symbol: None,
            funding_quote_symbol: None,
            funding_premium_symbol: None,
            funding_timestamp: None,
            funding_interval: None,
            funding_rate: None,
            indicative_funding_rate: None,
            funding_base_rate: None,
            funding_quote_rate: None,
            // Other fields
            calc_interval: None,
            publish_interval: None,
            publish_time: None,
            underlying_to_position_multiplier: Some(1.0),
            underlying_to_settle_multiplier: None,
            quote_to_settle_multiplier: Some(0.00000001),
            init_margin: Some(0.02),
            maint_margin: Some(0.01),
            risk_limit: Some(20000000000.0),
            risk_step: Some(10000000000.0),
            limit: None,
            taxed: Some(true),
            deleverage: Some(true),
            rebalance_timestamp: None,
            rebalance_interval: None,
            prev_close_price: Some(55000.0),
            limit_down_price: None,
            limit_up_price: None,
            prev_total_turnover: Some(10000000000000.0),
            home_notional_24h: Some(150.0),
            foreign_notional_24h: Some(7500000.0),
            prev_price_24h: Some(54500.0),
            vwap: Some(55100.0),
            high_price: Some(56000.0),
            low_price: Some(54000.0),
            last_price_protected: Some(55500.0),
            last_tick_direction: Some(BitmexTickDirection::PlusTick),
            last_change_pcnt: Some(0.0183),
            mid_price: Some(55500.0),
            impact_bid_price: Some(55490.0),
            impact_mid_price: Some(55495.0),
            impact_ask_price: Some(55500.0),
            fair_method: Some(BitmexFairMethod::ImpactMidPrice),
            fair_basis_rate: Some(1.8264),
            fair_basis: Some(1000.0),
            fair_price: Some(55500.0),
            mark_method: Some(BitmexMarkMethod::FairPrice),
            indicative_settle_price: Some(55500.0),
            settled_price_adjustment_rate: None,
            settled_price: None,
            instant_pnl: false,
            min_tick: None,
        }
    }

    // ========================================================================
    // Instrument Parsing Tests
    // ========================================================================

    #[rstest]
    fn test_parse_spot_instrument() {
        let instrument = create_test_spot_instrument();
        let ts_init = UnixNanos::default();
        let result = parse_spot_instrument(&instrument, ts_init).unwrap();

        // Check it's a CurrencyPair variant
        match result {
            nautilus_model::instruments::InstrumentAny::CurrencyPair(spot) => {
                assert_eq!(spot.id.symbol.as_str(), "XBTUSD");
                assert_eq!(spot.id.venue.as_str(), "BITMEX");
                assert_eq!(spot.raw_symbol.as_str(), "XBTUSD");
                assert_eq!(spot.price_precision, 2);
                assert_eq!(spot.size_precision, 0);
                assert_eq!(spot.price_increment.as_f64(), 0.01);
                assert_eq!(spot.size_increment.as_f64(), 1.0);
                assert_eq!(spot.maker_fee.to_f64().unwrap(), -0.00025);
                assert_eq!(spot.taker_fee.to_f64().unwrap(), 0.00075);
            }
            _ => panic!("Expected CurrencyPair variant"),
        }
    }

    #[rstest]
    fn test_parse_perpetual_instrument() {
        let instrument = create_test_perpetual_instrument();
        let ts_init = UnixNanos::default();
        let result = parse_perpetual_instrument(&instrument, ts_init).unwrap();

        // Check it's a CryptoPerpetual variant
        match result {
            nautilus_model::instruments::InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "XBTUSD");
                assert_eq!(perp.id.venue.as_str(), "BITMEX");
                assert_eq!(perp.raw_symbol.as_str(), "XBTUSD");
                assert_eq!(perp.price_precision, 1);
                assert_eq!(perp.size_precision, 0);
                assert_eq!(perp.price_increment.as_f64(), 0.5);
                assert_eq!(perp.size_increment.as_f64(), 1.0);
                assert_eq!(perp.maker_fee.to_f64().unwrap(), -0.00025);
                assert_eq!(perp.taker_fee.to_f64().unwrap(), 0.00075);
                assert!(perp.is_inverse);
            }
            _ => panic!("Expected CryptoPerpetual variant"),
        }
    }

    #[rstest]
    fn test_parse_futures_instrument() {
        let instrument = create_test_futures_instrument();
        let ts_init = UnixNanos::default();
        let result = parse_futures_instrument(&instrument, ts_init).unwrap();

        // Check it's a CryptoFuture variant
        match result {
            nautilus_model::instruments::InstrumentAny::CryptoFuture(instrument) => {
                assert_eq!(instrument.id.symbol.as_str(), "XBTH25");
                assert_eq!(instrument.id.venue.as_str(), "BITMEX");
                assert_eq!(instrument.raw_symbol.as_str(), "XBTH25");
                assert_eq!(instrument.underlying.code.as_str(), "XBT");
                assert_eq!(instrument.price_precision, 1);
                assert_eq!(instrument.size_precision, 0);
                assert_eq!(instrument.price_increment.as_f64(), 0.5);
                assert_eq!(instrument.size_increment.as_f64(), 1.0);
                assert_eq!(instrument.maker_fee.to_f64().unwrap(), -0.00025);
                assert_eq!(instrument.taker_fee.to_f64().unwrap(), 0.00075);
                assert!(instrument.is_inverse);
                // Check expiration timestamp instead of expiry_date
                // The futures contract expires on 2025-03-28
                assert!(instrument.expiration_ns.as_u64() > 0);
            }
            _ => panic!("Expected CryptoFuture variant"),
        }
    }
}
