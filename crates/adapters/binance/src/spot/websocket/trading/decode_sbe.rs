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

//! SBE decoders for Binance Spot user data stream events.
//!
//! Decodes templates 601 (BalanceUpdateEvent), 603 (ExecutionReportEvent), and
//! 607 (OutboundAccountPositionEvent) from schema 3:3 binary payloads into the
//! venue-level structs defined in [`super::user_data`].
//! The existing JSON parse functions in [`super::parse`] then convert these to Nautilus types.

use rust_decimal::Decimal;
use ustr::Ustr;

use super::user_data::{
    BinanceSpotAccountPositionMsg, BinanceSpotBalanceEntry, BinanceSpotBalanceUpdateMsg,
    BinanceSpotExecutionReport, BinanceSpotExecutionType,
};
use crate::{
    common::enums::{BinanceOrderStatus, BinanceSide, BinanceTimeInForce},
    spot::sbe::spot::{
        ReadBuf, balance_update_event_codec, bool_enum, execution_report_event_codec,
        execution_type, message_header_codec, order_side, order_status, order_type,
        outbound_account_position_event_codec, time_in_force,
    },
};

const HEADER_LEN: usize = message_header_codec::ENCODED_LENGTH;

/// Decodes an SBE ExecutionReportEvent (template 603) into a [`BinanceSpotExecutionReport`].
///
/// The input buffer must include the 8-byte SBE message header.
///
/// # Errors
///
/// Returns error if the buffer is too short, the template ID is wrong,
/// or the schema ID does not match.
pub fn decode_execution_report(data: &[u8]) -> anyhow::Result<BinanceSpotExecutionReport> {
    if data.len() < HEADER_LEN {
        anyhow::bail!(
            "Buffer too short for SBE header: expected {HEADER_LEN}, was {}",
            data.len()
        );
    }

    let buf = ReadBuf::new(data);
    let block_length = buf.get_u16_at(0);
    let template_id = buf.get_u16_at(2);
    let schema_id = buf.get_u16_at(4);
    let version = buf.get_u16_at(6);

    if template_id != execution_report_event_codec::SBE_TEMPLATE_ID {
        anyhow::bail!(
            "Wrong template ID: expected {}, received {template_id}",
            execution_report_event_codec::SBE_TEMPLATE_ID
        );
    }

    if schema_id != crate::spot::sbe::spot::SBE_SCHEMA_ID {
        anyhow::bail!(
            "Wrong schema ID: expected {}, received {schema_id}",
            crate::spot::sbe::spot::SBE_SCHEMA_ID
        );
    }

    let min_len = HEADER_LEN + block_length as usize;
    if data.len() < min_len {
        anyhow::bail!(
            "Buffer too short for fixed block: expected {min_len}, was {}",
            data.len()
        );
    }

    let mut dec = execution_report_event_codec::ExecutionReportEventDecoder::default().wrap(
        buf,
        HEADER_LEN,
        block_length,
        version,
    );

    let price_exp = dec.price_exponent();
    let qty_exp = dec.qty_exponent();
    let commission_exp = dec.commission_exponent();

    let event_time_us = dec.event_time();
    let transact_time_us = dec.transact_time();
    let order_creation_time_us = dec.order_creation_time();
    let order_id = dec.order_id();
    let trade_id = dec.trade_id().unwrap_or(-1);

    let execution_type = map_execution_type(dec.execution_type())?;
    let order_status = map_order_status(dec.order_status());
    let side = map_side(dec.side())?;
    let time_in_force = map_time_in_force(dec.time_in_force());
    let order_type_str = map_order_type(dec.order_type());
    let is_working = dec.is_working() == bool_enum::BoolEnum::True;
    let is_maker = dec.is_maker() == bool_enum::BoolEnum::True;

    let price_mantissa = dec.price();
    let orig_qty_mantissa = dec.orig_qty();
    let stop_price_mantissa = dec.stop_price();
    let last_qty_mantissa = dec.last_qty();
    let last_price_mantissa = dec.last_price();
    let executed_qty_mantissa = dec.executed_qty();
    let cummulative_quote_qty_mantissa = dec.cummulative_quote_qty();
    let commission_mantissa = dec.commission();

    // Variable-length fields must be read in schema order.
    // Each field is converted to an owned String immediately to release the borrow.
    let symbol = {
        let coords = dec.symbol_decoder();
        String::from_utf8_lossy(dec.symbol_slice(coords)).into_owned()
    };

    let client_order_id = {
        let coords = dec.client_order_id_decoder();
        String::from_utf8_lossy(dec.client_order_id_slice(coords)).into_owned()
    };

    let orig_client_order_id = {
        let coords = dec.orig_client_order_id_decoder();
        let s = String::from_utf8_lossy(dec.orig_client_order_id_slice(coords)).into_owned();
        if s.is_empty() { None } else { Some(s) }
    };

    let commission_asset = {
        let coords = dec.commission_asset_decoder();
        let bytes = dec.commission_asset_slice(coords);
        if bytes.is_empty() {
            None
        } else {
            Some(Ustr::from(&String::from_utf8_lossy(bytes)))
        }
    };

    let reject_reason = {
        let coords = dec.reject_reason_decoder();
        String::from_utf8_lossy(dec.reject_reason_slice(coords)).into_owned()
    };

    // counter_symbol - read to advance cursor, not used in venue struct
    let _counter_symbol_coords = dec.counter_symbol_decoder();

    Ok(BinanceSpotExecutionReport {
        event_type: "executionReport".to_string(),
        event_time: us_to_ms(event_time_us),
        symbol: Ustr::from(&symbol),
        client_order_id,
        side,
        order_type: order_type_str.to_string(),
        time_in_force,
        original_qty: mantissa_to_decimal_string(orig_qty_mantissa, qty_exp),
        price: mantissa_to_decimal_string(price_mantissa, price_exp),
        stop_price: mantissa_to_decimal_string(stop_price_mantissa, price_exp),
        execution_type,
        order_status,
        reject_reason,
        order_id,
        last_filled_qty: mantissa_to_decimal_string(last_qty_mantissa, qty_exp),
        cumulative_filled_qty: mantissa_to_decimal_string(executed_qty_mantissa, qty_exp),
        last_filled_price: mantissa_to_decimal_string(last_price_mantissa, price_exp),
        commission: mantissa_to_decimal_string(commission_mantissa, commission_exp),
        commission_asset,
        transaction_time: us_to_ms(transact_time_us),
        trade_id,
        is_working,
        is_maker,
        order_creation_time: order_creation_time_us.map_or(0, us_to_ms),
        cumulative_quote_qty: mantissa_to_decimal_string(
            cummulative_quote_qty_mantissa,
            price_exp + qty_exp,
        ),
        original_client_order_id: orig_client_order_id,
    })
}

/// Decodes an SBE OutboundAccountPositionEvent (template 607) into a
/// [`BinanceSpotAccountPositionMsg`].
///
/// The input buffer must include the 8-byte SBE message header.
///
/// # Errors
///
/// Returns error if the buffer is too short, the template ID is wrong,
/// or the schema ID does not match.
pub fn decode_account_position(data: &[u8]) -> anyhow::Result<BinanceSpotAccountPositionMsg> {
    if data.len() < HEADER_LEN {
        anyhow::bail!(
            "Buffer too short for SBE header: expected {HEADER_LEN}, was {}",
            data.len()
        );
    }

    let buf = ReadBuf::new(data);
    let block_length = buf.get_u16_at(0);
    let template_id = buf.get_u16_at(2);
    let schema_id = buf.get_u16_at(4);
    let version = buf.get_u16_at(6);

    if template_id != outbound_account_position_event_codec::SBE_TEMPLATE_ID {
        anyhow::bail!(
            "Wrong template ID: expected {}, received {template_id}",
            outbound_account_position_event_codec::SBE_TEMPLATE_ID
        );
    }

    if schema_id != crate::spot::sbe::spot::SBE_SCHEMA_ID {
        anyhow::bail!(
            "Wrong schema ID: expected {}, received {schema_id}",
            crate::spot::sbe::spot::SBE_SCHEMA_ID
        );
    }

    let min_len = HEADER_LEN + block_length as usize;
    if data.len() < min_len {
        anyhow::bail!(
            "Buffer too short for fixed block: expected {min_len}, was {}",
            data.len()
        );
    }

    let dec = outbound_account_position_event_codec::OutboundAccountPositionEventDecoder::default()
        .wrap(buf, HEADER_LEN, block_length, version);

    let event_time_us = dec.event_time();
    let update_time_us = dec.update_time();

    let mut balances_dec = dec.balances_decoder();
    let count = balances_dec.count() as usize;
    let mut balances = Vec::with_capacity(count);

    while let Some(_idx) = balances_dec
        .advance()
        .map_err(|e| anyhow::anyhow!("Failed to advance balances group: {e:?}"))?
    {
        let exponent = balances_dec.exponent();
        let free_mantissa = balances_dec.free();
        let locked_mantissa = balances_dec.locked();

        let asset_coords = balances_dec.asset_decoder();
        let asset_bytes = balances_dec.asset_slice(asset_coords);
        let asset = Ustr::from(&String::from_utf8_lossy(asset_bytes));

        balances.push(BinanceSpotBalanceEntry {
            asset,
            free: mantissa_to_decimal(free_mantissa, exponent),
            locked: mantissa_to_decimal(locked_mantissa, exponent),
        });
    }

    Ok(BinanceSpotAccountPositionMsg {
        event_type: "outboundAccountPosition".to_string(),
        event_time: us_to_ms(event_time_us),
        last_update_time: us_to_ms(update_time_us),
        balances,
    })
}

/// Decodes an SBE BalanceUpdateEvent (template 601) into a [`BinanceSpotBalanceUpdateMsg`].
///
/// The input buffer must include the 8-byte SBE message header.
///
/// # Errors
///
/// Returns error if the buffer is too short, the template ID is wrong,
/// or the schema ID does not match.
pub fn decode_balance_update(data: &[u8]) -> anyhow::Result<BinanceSpotBalanceUpdateMsg> {
    if data.len() < HEADER_LEN {
        anyhow::bail!(
            "Buffer too short for SBE header: expected {HEADER_LEN}, was {}",
            data.len()
        );
    }

    let buf = ReadBuf::new(data);
    let block_length = buf.get_u16_at(0);
    let template_id = buf.get_u16_at(2);
    let schema_id = buf.get_u16_at(4);
    let version = buf.get_u16_at(6);

    if template_id != balance_update_event_codec::SBE_TEMPLATE_ID {
        anyhow::bail!(
            "Wrong template ID: expected {}, received {template_id}",
            balance_update_event_codec::SBE_TEMPLATE_ID
        );
    }

    if schema_id != crate::spot::sbe::spot::SBE_SCHEMA_ID {
        anyhow::bail!(
            "Wrong schema ID: expected {}, received {schema_id}",
            crate::spot::sbe::spot::SBE_SCHEMA_ID
        );
    }

    let min_len = HEADER_LEN + block_length as usize;
    if data.len() < min_len {
        anyhow::bail!(
            "Buffer too short for fixed block: expected {min_len}, was {}",
            data.len()
        );
    }

    let mut dec = balance_update_event_codec::BalanceUpdateEventDecoder::default().wrap(
        buf,
        HEADER_LEN,
        block_length,
        version,
    );

    let event_time_us = dec.event_time();
    let clear_time_us = dec.clear_time().unwrap_or(0);
    let qty_exponent = dec.qty_exponent();
    let free_qty_delta = dec.free_qty_delta();

    let asset = {
        let coords = dec.asset_decoder();
        String::from_utf8_lossy(dec.asset_slice(coords)).into_owned()
    };

    Ok(BinanceSpotBalanceUpdateMsg {
        event_type: "balanceUpdate".to_string(),
        event_time: us_to_ms(event_time_us),
        asset: Ustr::from(&asset),
        delta: mantissa_to_decimal_string(free_qty_delta, qty_exponent),
        clear_time: us_to_ms(clear_time_us),
    })
}

fn map_execution_type(
    et: execution_type::ExecutionType,
) -> anyhow::Result<BinanceSpotExecutionType> {
    match et {
        execution_type::ExecutionType::New => Ok(BinanceSpotExecutionType::New),
        execution_type::ExecutionType::Canceled => Ok(BinanceSpotExecutionType::Canceled),
        execution_type::ExecutionType::Replaced => Ok(BinanceSpotExecutionType::Replaced),
        execution_type::ExecutionType::Rejected => Ok(BinanceSpotExecutionType::Rejected),
        execution_type::ExecutionType::Trade => Ok(BinanceSpotExecutionType::Trade),
        execution_type::ExecutionType::Expired => Ok(BinanceSpotExecutionType::Expired),
        execution_type::ExecutionType::TradePrevention => {
            Ok(BinanceSpotExecutionType::TradePrevention)
        }
        _ => anyhow::bail!("Unsupported SBE execution type: {et}"),
    }
}

fn map_order_status(os: order_status::OrderStatus) -> BinanceOrderStatus {
    match os {
        order_status::OrderStatus::New => BinanceOrderStatus::New,
        order_status::OrderStatus::PartiallyFilled => BinanceOrderStatus::PartiallyFilled,
        order_status::OrderStatus::Filled => BinanceOrderStatus::Filled,
        order_status::OrderStatus::Canceled => BinanceOrderStatus::Canceled,
        order_status::OrderStatus::PendingCancel => BinanceOrderStatus::PendingCancel,
        order_status::OrderStatus::Rejected => BinanceOrderStatus::Rejected,
        order_status::OrderStatus::Expired => BinanceOrderStatus::Expired,
        order_status::OrderStatus::ExpiredInMatch => BinanceOrderStatus::ExpiredInMatch,
        _ => BinanceOrderStatus::Unknown,
    }
}

fn map_side(side: order_side::OrderSide) -> anyhow::Result<BinanceSide> {
    match side {
        order_side::OrderSide::Buy => Ok(BinanceSide::Buy),
        order_side::OrderSide::Sell => Ok(BinanceSide::Sell),
        _ => anyhow::bail!("Unsupported SBE order side: {side}"),
    }
}

fn map_time_in_force(tif: time_in_force::TimeInForce) -> BinanceTimeInForce {
    match tif {
        time_in_force::TimeInForce::Gtc => BinanceTimeInForce::Gtc,
        time_in_force::TimeInForce::Ioc => BinanceTimeInForce::Ioc,
        time_in_force::TimeInForce::Fok => BinanceTimeInForce::Fok,
        _ => BinanceTimeInForce::Unknown,
    }
}

fn map_order_type(ot: order_type::OrderType) -> &'static str {
    match ot {
        order_type::OrderType::Market => "MARKET",
        order_type::OrderType::Limit => "LIMIT",
        order_type::OrderType::StopLoss => "STOP_LOSS",
        order_type::OrderType::StopLossLimit => "STOP_LOSS_LIMIT",
        order_type::OrderType::TakeProfit => "TAKE_PROFIT",
        order_type::OrderType::TakeProfitLimit => "TAKE_PROFIT_LIMIT",
        order_type::OrderType::LimitMaker => "LIMIT_MAKER",
        _ => "UNKNOWN",
    }
}

/// Converts SBE microsecond timestamp to JSON millisecond timestamp.
#[inline]
fn us_to_ms(us: i64) -> i64 {
    us / 1000
}

/// Converts an SBE `mantissa * 10^exponent` pair to a [`Decimal`] without floating-point.
fn mantissa_to_decimal(mantissa: i64, exponent: i8) -> Decimal {
    if exponent >= 0 {
        Decimal::from(mantissa) * Decimal::from(10_i64.pow(exponent as u32))
    } else {
        Decimal::new(mantissa, (-exponent) as u32)
    }
}

/// Converts a mantissa + exponent pair to a decimal string without floating-point.
///
/// SBE encodes numeric values as `mantissa * 10^exponent`. For exponent = -2
/// and mantissa = 250000, the result is "2500.00".
fn mantissa_to_decimal_string(mantissa: i64, exponent: i8) -> String {
    if mantissa == 0 {
        if exponent >= 0 {
            return "0".to_string();
        }
        let mut s = "0.".to_string();
        for _ in 0..(-exponent) {
            s.push('0');
        }
        return s;
    }

    let negative = mantissa < 0;
    let abs_mantissa = mantissa.unsigned_abs();
    let digits = abs_mantissa.to_string();

    let result = if exponent >= 0 {
        let mut s = digits;
        for _ in 0..exponent {
            s.push('0');
        }
        s
    } else {
        let decimal_places = (-exponent) as usize;
        if digits.len() <= decimal_places {
            let padding = decimal_places - digits.len();
            let mut s = "0.".to_string();
            for _ in 0..padding {
                s.push('0');
            }
            s.push_str(&digits);
            s
        } else {
            let split_pos = digits.len() - decimal_places;
            let mut s = digits[..split_pos].to_string();
            s.push('.');
            s.push_str(&digits[split_pos..]);
            s
        }
    };

    if negative {
        format!("-{result}")
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::spot::sbe::spot::{
        WriteBuf, bool_enum::BoolEnum, execution_type::ExecutionType, floor, match_type,
        order_capacity, order_side::OrderSide, order_status::OrderStatus,
        order_type::OrderType as SbeOrderType, peg_offset_type, peg_price_type,
        self_trade_prevention_mode::SelfTradePreventionMode, time_in_force::TimeInForce as SbeTif,
    };

    #[expect(clippy::too_many_arguments)]
    fn encode_execution_report(
        symbol: &str,
        client_order_id: &str,
        order_id: i64,
        trade_id: Option<i64>,
        side: OrderSide,
        order_type: SbeOrderType,
        tif: SbeTif,
        exec_type: ExecutionType,
        status: OrderStatus,
        price_exp: i8,
        qty_exp: i8,
        commission_exp: i8,
        price_mantissa: i64,
        orig_qty_mantissa: i64,
        stop_price_mantissa: i64,
        last_qty_mantissa: i64,
        last_price_mantissa: i64,
        executed_qty_mantissa: i64,
        cumm_quote_qty_mantissa: i64,
        commission_mantissa: i64,
        commission_asset: &str,
        is_maker: bool,
        is_working: bool,
        event_time_us: i64,
        transact_time_us: i64,
        order_creation_time_us: Option<i64>,
    ) -> Vec<u8> {
        let var_data_len = 6 + symbol.len() + client_order_id.len() + commission_asset.len();
        let total = 8 + execution_report_event_codec::SBE_BLOCK_LENGTH as usize + var_data_len;
        let mut buf_vec = vec![0u8; total];

        let buf = WriteBuf::new(buf_vec.as_mut_slice());
        let enc = execution_report_event_codec::ExecutionReportEventEncoder::default()
            .wrap(buf, HEADER_LEN);
        let mut header = enc.header(0);
        let mut enc = header.parent().unwrap();

        enc.event_time(event_time_us);
        enc.transact_time(transact_time_us);
        enc.price_exponent(price_exp);
        enc.qty_exponent(qty_exp);
        enc.commission_exponent(commission_exp);
        enc.order_creation_time(order_creation_time_us.unwrap_or(i64::MIN));
        enc.working_time(i64::MIN); // null
        enc.order_id(order_id);
        enc.order_list_id(i64::MIN); // null
        enc.orig_qty(orig_qty_mantissa);
        enc.price(price_mantissa);
        enc.orig_quote_order_qty(0);
        enc.iceberg_qty(0);
        enc.stop_price(stop_price_mantissa);
        enc.order_type(order_type);
        enc.side(side);
        enc.time_in_force(tif);
        enc.execution_type(exec_type);
        enc.order_status(status);
        enc.trade_id(trade_id.unwrap_or(i64::MIN));
        enc.execution_id(0);
        enc.executed_qty(executed_qty_mantissa);
        enc.cummulative_quote_qty(cumm_quote_qty_mantissa);
        enc.last_qty(last_qty_mantissa);
        enc.last_price(last_price_mantissa);
        enc.quote_qty(0);
        enc.commission(commission_mantissa);
        enc.is_working(if is_working {
            BoolEnum::True
        } else {
            BoolEnum::False
        });
        enc.is_maker(if is_maker {
            BoolEnum::True
        } else {
            BoolEnum::False
        });
        enc.is_best_match(BoolEnum::False);
        enc.match_type(match_type::MatchType::default());
        enc.self_trade_prevention_mode(SelfTradePreventionMode::default());
        enc.order_capacity(order_capacity::OrderCapacity::default());
        enc.working_floor(floor::Floor::default());
        enc.used_sor(BoolEnum::False);
        enc.alloc_id(i64::MIN);
        enc.trailing_delta(u64::MAX);
        enc.trailing_time(i64::MIN);
        enc.trade_group_id(i64::MIN);
        enc.prevented_qty(0);
        enc.last_prevented_qty(i64::MIN);
        enc.prevented_match_id(i64::MIN);
        enc.prevented_execution_qty(i64::MIN);
        enc.prevented_execution_price(i64::MIN);
        enc.prevented_execution_quote_qty(i64::MIN);
        enc.strategy_type(i32::MIN);
        enc.strategy_id(i64::MIN);
        enc.counter_order_id(i64::MIN);
        enc.subscription_id(0xFFFF); // null
        enc.peg_price_type(peg_price_type::PegPriceType::default());
        enc.peg_offset_type(peg_offset_type::PegOffsetType::default());
        enc.peg_offset_value(0xFF); // null
        enc.pegged_price(i64::MIN);

        // Variable-length fields in order
        enc.symbol(symbol);
        enc.client_order_id(client_order_id);
        enc.orig_client_order_id("");
        enc.commission_asset(commission_asset);
        enc.reject_reason("");
        enc.counter_symbol("");

        buf_vec
    }

    fn encode_account_position(
        event_time_us: i64,
        update_time_us: i64,
        balances: &[(&str, i8, i64, i64)], // (asset, exponent, free, locked)
    ) -> Vec<u8> {
        let var_data_len: usize = balances.iter().map(|(a, _, _, _)| 1 + a.len()).sum();
        let total = 8 + 18 + 6 + (balances.len() * 17) + var_data_len;
        let mut buf_vec = vec![0u8; total];

        let buf = WriteBuf::new(buf_vec.as_mut_slice());
        let enc =
            outbound_account_position_event_codec::OutboundAccountPositionEventEncoder::default()
                .wrap(buf, HEADER_LEN);
        let mut header = enc.header(0);
        let mut enc = header.parent().unwrap();

        enc.event_time(event_time_us);
        enc.update_time(update_time_us);
        enc.subscription_id(0xFFFF); // null

        let balances_enc =
            outbound_account_position_event_codec::encoder::BalancesEncoder::default();
        let mut bal_enc = enc.balances_encoder(balances.len() as u32, balances_enc);

        for (asset, exponent, free, locked) in balances {
            bal_enc.advance().unwrap();
            bal_enc.exponent(*exponent);
            bal_enc.free(*free);
            bal_enc.locked(*locked);
            bal_enc.asset(asset);
        }

        buf_vec
    }

    #[rstest]
    fn test_mantissa_to_decimal_string_basic() {
        assert_eq!(mantissa_to_decimal_string(250000, -2), "2500.00");
        assert_eq!(mantissa_to_decimal_string(100000000, -8), "1.00000000");
        assert_eq!(mantissa_to_decimal_string(0, -8), "0.00000000");
        assert_eq!(mantissa_to_decimal_string(0, 0), "0");
        assert_eq!(mantissa_to_decimal_string(42, 0), "42");
        assert_eq!(mantissa_to_decimal_string(42, 2), "4200");
        assert_eq!(mantissa_to_decimal_string(5, -3), "0.005");
        assert_eq!(mantissa_to_decimal_string(-250000, -2), "-2500.00");
    }

    #[rstest]
    #[case::typical_price(250000_i64, -2_i8, "2500.00")]
    #[case::btc_one(100000000_i64, -8_i8, "1.00000000")]
    #[case::zero_negative_exp(0_i64, -8_i8, "0")]
    #[case::zero_zero_exp(0_i64, 0_i8, "0")]
    #[case::whole_no_scale(42_i64, 0_i8, "42")]
    #[case::positive_exponent(42_i64, 2_i8, "4200")]
    #[case::small_fractional(5_i64, -3_i8, "0.005")]
    #[case::negative_mantissa(-250000_i64, -2_i8, "-2500.00")]
    #[case::large_positive_exponent(1_i64, 9_i8, "1000000000")]
    fn test_mantissa_to_decimal_parametrized(
        #[case] mantissa: i64,
        #[case] exponent: i8,
        #[case] expected: &str,
    ) {
        let result = mantissa_to_decimal(mantissa, exponent);
        assert_eq!(result, Decimal::from_str_exact(expected).unwrap());
    }

    #[rstest]
    fn test_decode_execution_report_new_limit() {
        let data = encode_execution_report(
            "ETHUSDT",
            "O-20200101-000000-000-000-0",
            12345678,
            None, // no trade
            OrderSide::Buy,
            SbeOrderType::Limit,
            SbeTif::Gtc,
            ExecutionType::New,
            OrderStatus::New,
            -2,     // price_exp
            -5,     // qty_exp
            -8,     // commission_exp
            250000, // price = 2500.00
            100000, // orig_qty = 1.00000
            0,      // stop_price
            0,      // last_qty
            0,      // last_price
            0,      // executed_qty
            0,      // cumm_quote_qty
            0,      // commission
            "",     // no commission asset yet
            false,
            true,             // is_working
            1709654400000000, // event_time_us
            1709654400000000, // transact_time_us
            Some(1709654400000000),
        );

        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.symbol, "ETHUSDT");
        assert_eq!(report.client_order_id, "O-20200101-000000-000-000-0");
        assert_eq!(report.order_id, 12345678);
        assert_eq!(report.side, BinanceSide::Buy);
        assert_eq!(report.order_type, "LIMIT");
        assert_eq!(report.time_in_force, BinanceTimeInForce::Gtc);
        assert_eq!(report.execution_type, BinanceSpotExecutionType::New);
        assert_eq!(report.order_status, BinanceOrderStatus::New);
        assert_eq!(report.price, "2500.00");
        assert_eq!(report.original_qty, "1.00000");
        assert_eq!(report.trade_id, -1);
        assert!(report.is_working);
        assert!(!report.is_maker);
        assert_eq!(report.event_time, 1709654400000);
        assert_eq!(report.transaction_time, 1709654400000);
    }

    #[rstest]
    fn test_decode_execution_report_trade_fill() {
        let data = encode_execution_report(
            "ETHUSDT",
            "O-20200101-000000-000-000-0",
            12345678,
            Some(98765432),
            OrderSide::Buy,
            SbeOrderType::Limit,
            SbeTif::Gtc,
            ExecutionType::Trade,
            OrderStatus::Filled,
            -2,           // price_exp
            -8,           // qty_exp
            -8,           // commission_exp
            250000,       // price = 2500.00
            100000000,    // orig_qty = 1.00000000
            0,            // stop_price
            100000000,    // last_qty = 1.00000000
            250000,       // last_price = 2500.00 (uses price_exp)
            100000000,    // executed_qty = 1.00000000
            250000000000, // cumm_quote_qty = 2500.00000000 (uses price_exp... actually this is wrong)
            250000,       // commission = 0.00250000
            "USDT",
            true, // is_maker
            false,
            1709654400000000,
            1709654400000000,
            Some(1709654400000000),
        );

        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.execution_type, BinanceSpotExecutionType::Trade);
        assert_eq!(report.order_status, BinanceOrderStatus::Filled);
        assert_eq!(report.trade_id, 98765432);
        assert_eq!(report.last_filled_qty, "1.00000000");
        assert_eq!(report.last_filled_price, "2500.00");
        assert_eq!(report.commission_asset, Some(Ustr::from("USDT")));
        assert!(report.is_maker);
    }

    #[rstest]
    fn test_decode_execution_report_canceled() {
        let data = encode_execution_report(
            "BTCUSDT",
            "O-20200101-000000-000-000-1",
            99999,
            None,
            OrderSide::Sell,
            SbeOrderType::Limit,
            SbeTif::Gtc,
            ExecutionType::Canceled,
            OrderStatus::Canceled,
            -2,
            -8,
            -8,
            5000000,  // price = 50000.00
            10000000, // orig_qty = 0.10000000
            0,
            0,
            0,
            0,
            0,
            0,
            "",
            false,
            false,
            1709654400000000,
            1709654400000000,
            Some(1709654400000000),
        );

        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.execution_type, BinanceSpotExecutionType::Canceled);
        assert_eq!(report.order_status, BinanceOrderStatus::Canceled);
        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.side, BinanceSide::Sell);
    }

    #[rstest]
    fn test_decode_execution_report_stop_loss_limit() {
        let data = encode_execution_report(
            "ETHUSDT",
            "O-20200101-000000-000-000-1",
            12345679,
            None,
            OrderSide::Sell,
            SbeOrderType::StopLossLimit,
            SbeTif::Gtc,
            ExecutionType::New,
            OrderStatus::New,
            -2,
            -5,
            -8,
            240000, // price = 2400.00
            100000, // orig_qty = 1.00000
            245000, // stop_price = 2450.00
            0,
            0,
            0,
            0,
            0,
            "",
            false,
            true,
            1709654400000000,
            1709654400000000,
            Some(1709654400000000),
        );

        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.order_type, "STOP_LOSS_LIMIT");
        assert_eq!(report.price, "2400.00");
        assert_eq!(report.stop_price, "2450.00");
    }

    #[rstest]
    fn test_decode_execution_report_truncated_header() {
        let data = vec![0u8; 5];
        let err = decode_execution_report(&data).unwrap_err();
        assert!(err.to_string().contains("too short for SBE header"));
    }

    #[rstest]
    fn test_decode_execution_report_wrong_template() {
        let mut data = encode_execution_report(
            "TEST",
            "test",
            1,
            None,
            OrderSide::Buy,
            SbeOrderType::Limit,
            SbeTif::Gtc,
            ExecutionType::New,
            OrderStatus::New,
            -2,
            -8,
            -8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            "",
            false,
            false,
            0,
            0,
            None,
        );
        // Overwrite template_id to 50
        data[2..4].copy_from_slice(&50u16.to_le_bytes());

        let err = decode_execution_report(&data).unwrap_err();
        assert!(err.to_string().contains("Wrong template ID"));
    }

    #[rstest]
    fn test_decode_account_position_single_balance() {
        let data = encode_account_position(
            1709654400000000,                            // event_time_us
            1709654400000000,                            // update_time_us
            &[("USDT", -8, 1000000000000, 50000000000)], // free=10000.00000000, locked=500.00000000
        );

        let msg = decode_account_position(&data).unwrap();

        assert_eq!(msg.event_type, "outboundAccountPosition");
        assert_eq!(msg.event_time, 1709654400000);
        assert_eq!(msg.balances.len(), 1);
        assert_eq!(msg.balances[0].asset, "USDT");
        assert_eq!(
            msg.balances[0].free,
            Decimal::from_str_exact("10000.00000000").unwrap()
        );
        assert_eq!(
            msg.balances[0].locked,
            Decimal::from_str_exact("500.00000000").unwrap()
        );
    }

    #[rstest]
    fn test_decode_account_position_multiple_balances() {
        let data = encode_account_position(
            1709654400000000,
            1709654400000000,
            &[
                ("BTC", -8, 100000000, 0),      // free=1.00000000, locked=0.00000000
                ("USDT", -8, 5000000000000, 0), // free=50000.00000000, locked=0.00000000
            ],
        );

        let msg = decode_account_position(&data).unwrap();

        assert_eq!(msg.balances.len(), 2);
        assert_eq!(msg.balances[0].asset, "BTC");
        assert_eq!(
            msg.balances[0].free,
            Decimal::from_str_exact("1.00000000").unwrap()
        );
        assert_eq!(msg.balances[1].asset, "USDT");
        assert_eq!(
            msg.balances[1].free,
            Decimal::from_str_exact("50000.00000000").unwrap()
        );
    }

    #[rstest]
    fn test_decode_account_position_zero_balances() {
        let data = encode_account_position(1709654400000000, 1709654400000000, &[]);

        let msg = decode_account_position(&data).unwrap();
        assert!(msg.balances.is_empty());
    }

    #[rstest]
    fn test_decode_account_position_truncated_header() {
        let data = vec![0u8; 5];
        let err = decode_account_position(&data).unwrap_err();
        assert!(err.to_string().contains("too short for SBE header"));
    }

    #[rstest]
    fn test_decode_account_position_wrong_template() {
        let mut data = encode_account_position(0, 0, &[]);
        data[2..4].copy_from_slice(&50u16.to_le_bytes());

        let err = decode_account_position(&data).unwrap_err();
        assert!(err.to_string().contains("Wrong template ID"));
    }

    fn encode_balance_update(
        event_time_us: i64,
        clear_time_us: i64,
        qty_exponent: i8,
        free_qty_delta: i64,
        asset: &str,
    ) -> Vec<u8> {
        let total = 8 + 27 + 1 + asset.len();
        let mut buf_vec = vec![0u8; total];

        let buf = WriteBuf::new(buf_vec.as_mut_slice());
        let enc =
            balance_update_event_codec::BalanceUpdateEventEncoder::default().wrap(buf, HEADER_LEN);
        let mut header = enc.header(0);
        let mut enc = header.parent().unwrap();

        enc.event_time(event_time_us);
        enc.clear_time(clear_time_us);
        enc.qty_exponent(qty_exponent);
        enc.free_qty_delta(free_qty_delta);
        enc.subscription_id(0xFFFF); // null
        enc.asset(asset);

        buf_vec
    }

    #[rstest]
    fn test_decode_balance_update() {
        let data = encode_balance_update(
            1709654400000000, // event_time_us
            1709654400000000, // clear_time_us
            -8,
            10000000000, // delta = 100.00000000
            "BTC",
        );

        let msg = decode_balance_update(&data).unwrap();

        assert_eq!(msg.event_type, "balanceUpdate");
        assert_eq!(msg.event_time, 1709654400000);
        assert_eq!(msg.asset, "BTC");
        assert_eq!(msg.delta, "100.00000000");
        assert_eq!(msg.clear_time, 1709654400000);
    }

    #[rstest]
    fn test_decode_balance_update_truncated_header() {
        let data = vec![0u8; 5];
        let err = decode_balance_update(&data).unwrap_err();
        assert!(err.to_string().contains("too short for SBE header"));
    }

    #[rstest]
    fn test_decode_balance_update_wrong_template() {
        let mut data = encode_balance_update(0, 0, -8, 0, "BTC");
        data[2..4].copy_from_slice(&50u16.to_le_bytes());

        let err = decode_balance_update(&data).unwrap_err();
        assert!(err.to_string().contains("Wrong template ID"));
    }

    #[rstest]
    fn test_us_to_ms() {
        assert_eq!(us_to_ms(1709654400000000), 1709654400000);
        assert_eq!(us_to_ms(1709654400123456), 1709654400123);
    }

    #[rstest]
    fn test_decode_captured_execution_report_new() {
        let data = crate::common::testing::load_fixture_bytes(
            "spot/user_data_sbe/mainnet/execution_report_event_1.sbe",
        );
        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.client_order_id, "O-20200101-000000-000-000-0");
        assert_eq!(report.execution_type, BinanceSpotExecutionType::New);
        assert_eq!(report.order_status, BinanceOrderStatus::New);
        assert_eq!(report.side, BinanceSide::Buy);
        assert_eq!(report.order_type, "LIMIT");
        assert_eq!(report.time_in_force, BinanceTimeInForce::Gtc);
        assert_eq!(report.order_id, 12345678);
        assert!(report.is_working);
        assert!(!report.is_maker);
        assert_eq!(report.trade_id, -1);
    }

    #[rstest]
    fn test_decode_captured_execution_report_canceled() {
        let data = crate::common::testing::load_fixture_bytes(
            "spot/user_data_sbe/mainnet/execution_report_event_2.sbe",
        );
        let report = decode_execution_report(&data).unwrap();

        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.execution_type, BinanceSpotExecutionType::Canceled);
        assert_eq!(report.order_status, BinanceOrderStatus::Canceled);
        assert_eq!(report.order_id, 12345678);
        assert!(!report.is_working);
    }

    #[rstest]
    fn test_decode_captured_account_position() {
        let data = crate::common::testing::load_fixture_bytes(
            "spot/user_data_sbe/mainnet/outbound_account_position_event_1.sbe",
        );
        let msg = decode_account_position(&data).unwrap();

        assert_eq!(msg.event_type, "outboundAccountPosition");
        assert_eq!(msg.balances.len(), 3);
        assert_eq!(msg.balances[0].asset, "BTC");
        assert_eq!(
            msg.balances[0].free,
            Decimal::from_str_exact("1.00000000").unwrap()
        );
        assert_eq!(msg.balances[1].asset, "BNB");
        assert_eq!(msg.balances[2].asset, "USDT");
        assert_eq!(
            msg.balances[2].free,
            Decimal::from_str_exact("50000.00000000").unwrap()
        );
    }
}
