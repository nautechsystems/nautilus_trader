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

//! SBE decode functions for Binance Spot HTTP responses.
//!
//! Each function decodes raw SBE bytes into domain types, validating the
//! message header (schema ID, version, template ID) before extracting fields.

use super::{
    error::SbeDecodeError,
    models::{
        BinanceAccountInfo, BinanceAccountTrade, BinanceBalance, BinanceCancelOrderResponse,
        BinanceDepth, BinanceExchangeInfoSbe, BinanceKline, BinanceKlines, BinanceLotSizeFilterSbe,
        BinanceNewOrderResponse, BinanceOrderFill, BinanceOrderResponse, BinancePriceFilterSbe,
        BinancePriceLevel, BinanceSymbolFiltersSbe, BinanceSymbolSbe, BinanceTrade, BinanceTrades,
    },
};
use crate::common::sbe::{
    cursor::SbeCursor,
    spot::{
        SBE_SCHEMA_ID, SBE_SCHEMA_VERSION,
        account_response_codec::SBE_TEMPLATE_ID as ACCOUNT_TEMPLATE_ID,
        account_trades_response_codec::SBE_TEMPLATE_ID as ACCOUNT_TRADES_TEMPLATE_ID,
        account_type::AccountType, bool_enum::BoolEnum,
        cancel_open_orders_response_codec::SBE_TEMPLATE_ID as CANCEL_OPEN_ORDERS_TEMPLATE_ID,
        cancel_order_response_codec::SBE_TEMPLATE_ID as CANCEL_ORDER_TEMPLATE_ID,
        depth_response_codec::SBE_TEMPLATE_ID as DEPTH_TEMPLATE_ID,
        exchange_info_response_codec::SBE_TEMPLATE_ID as EXCHANGE_INFO_TEMPLATE_ID,
        klines_response_codec::SBE_TEMPLATE_ID as KLINES_TEMPLATE_ID,
        lot_size_filter_codec::SBE_TEMPLATE_ID as LOT_SIZE_FILTER_TEMPLATE_ID,
        message_header_codec::ENCODED_LENGTH as HEADER_LENGTH,
        new_order_full_response_codec::SBE_TEMPLATE_ID as NEW_ORDER_FULL_TEMPLATE_ID,
        order_response_codec::SBE_TEMPLATE_ID as ORDER_TEMPLATE_ID,
        orders_response_codec::SBE_TEMPLATE_ID as ORDERS_TEMPLATE_ID,
        ping_response_codec::SBE_TEMPLATE_ID as PING_TEMPLATE_ID,
        price_filter_codec::SBE_TEMPLATE_ID as PRICE_FILTER_TEMPLATE_ID,
        server_time_response_codec::SBE_TEMPLATE_ID as SERVER_TIME_TEMPLATE_ID,
        trades_response_codec::SBE_TEMPLATE_ID as TRADES_TEMPLATE_ID,
    },
};

/// SBE message header.
#[derive(Debug, Clone, Copy)]
struct MessageHeader {
    #[allow(dead_code)]
    block_length: u16,
    template_id: u16,
    schema_id: u16,
    version: u16,
}

impl MessageHeader {
    /// Decode message header using cursor.
    fn decode_cursor(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        cursor.require(HEADER_LENGTH)?;
        Ok(Self {
            block_length: cursor.read_u16_le()?,
            template_id: cursor.read_u16_le()?,
            schema_id: cursor.read_u16_le()?,
            version: cursor.read_u16_le()?,
        })
    }

    /// Validate schema ID and version.
    fn validate(&self) -> Result<(), SbeDecodeError> {
        if self.schema_id != SBE_SCHEMA_ID {
            return Err(SbeDecodeError::SchemaMismatch {
                expected: SBE_SCHEMA_ID,
                actual: self.schema_id,
            });
        }
        if self.version != SBE_SCHEMA_VERSION {
            return Err(SbeDecodeError::VersionMismatch {
                expected: SBE_SCHEMA_VERSION,
                actual: self.version,
            });
        }
        Ok(())
    }
}

/// Decode a ping response.
///
/// Ping response has no body (block_length = 0), just validates the header.
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
pub fn decode_ping(buf: &[u8]) -> Result<(), SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != PING_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    Ok(())
}

/// Decode a server time response.
///
/// Returns the server time as **microseconds** since epoch (SBE provides
/// microsecond precision vs JSON's milliseconds).
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
pub fn decode_server_time(buf: &[u8]) -> Result<i64, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != SERVER_TIME_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    cursor.read_i64_le()
}

/// Decode a depth response.
///
/// Returns the order book depth with bids and asks.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
pub fn decode_depth(buf: &[u8]) -> Result<BinanceDepth, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != DEPTH_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let last_update_id = cursor.read_i64_le()?;
    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;

    let (block_len, count) = cursor.read_group_header()?;
    let bids = cursor.read_group(block_len, count, |c| {
        Ok(BinancePriceLevel {
            price_mantissa: c.read_i64_le()?,
            qty_mantissa: c.read_i64_le()?,
        })
    })?;

    let (block_len, count) = cursor.read_group_header()?;
    let asks = cursor.read_group(block_len, count, |c| {
        Ok(BinancePriceLevel {
            price_mantissa: c.read_i64_le()?,
            qty_mantissa: c.read_i64_le()?,
        })
    })?;

    Ok(BinanceDepth {
        last_update_id,
        price_exponent,
        qty_exponent,
        bids,
        asks,
    })
}

/// Decode a trades response.
///
/// Returns the list of trades.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
pub fn decode_trades(buf: &[u8]) -> Result<BinanceTrades, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != TRADES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;

    let (block_len, count) = cursor.read_group_header()?;
    let trades = cursor.read_group(block_len, count, |c| {
        Ok(BinanceTrade {
            id: c.read_i64_le()?,
            price_mantissa: c.read_i64_le()?,
            qty_mantissa: c.read_i64_le()?,
            quote_qty_mantissa: c.read_i64_le()?,
            time: c.read_i64_le()?,
            is_buyer_maker: BoolEnum::from(c.read_u8()?) == BoolEnum::True,
            is_best_match: BoolEnum::from(c.read_u8()?) == BoolEnum::True,
        })
    })?;

    Ok(BinanceTrades {
        price_exponent,
        qty_exponent,
        trades,
    })
}

/// Klines group item block length (from SBE codec).
const KLINES_BLOCK_LENGTH: u16 = 120;

/// Decode a klines (candlestick) response.
///
/// Returns the list of klines with their price and quantity exponents.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
pub fn decode_klines(buf: &[u8]) -> Result<BinanceKlines, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != KLINES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;

    let (block_len, count) = cursor.read_group_header()?;

    if block_len != KLINES_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: KLINES_BLOCK_LENGTH,
            actual: block_len,
        });
    }

    let mut klines = Vec::with_capacity(count as usize);

    for _ in 0..count {
        cursor.require(KLINES_BLOCK_LENGTH as usize)?;

        let open_time = cursor.read_i64_le()?;
        let open_price = cursor.read_i64_le()?;
        let high_price = cursor.read_i64_le()?;
        let low_price = cursor.read_i64_le()?;
        let close_price = cursor.read_i64_le()?;

        let volume_slice = cursor.read_bytes(16)?;
        let mut volume = [0u8; 16];
        volume.copy_from_slice(volume_slice);

        let close_time = cursor.read_i64_le()?;

        let quote_volume_slice = cursor.read_bytes(16)?;
        let mut quote_volume = [0u8; 16];
        quote_volume.copy_from_slice(quote_volume_slice);

        let num_trades = cursor.read_i64_le()?;

        let taker_buy_base_volume_slice = cursor.read_bytes(16)?;
        let mut taker_buy_base_volume = [0u8; 16];
        taker_buy_base_volume.copy_from_slice(taker_buy_base_volume_slice);

        let taker_buy_quote_volume_slice = cursor.read_bytes(16)?;
        let mut taker_buy_quote_volume = [0u8; 16];
        taker_buy_quote_volume.copy_from_slice(taker_buy_quote_volume_slice);

        klines.push(BinanceKline {
            open_time,
            open_price,
            high_price,
            low_price,
            close_price,
            volume,
            close_time,
            quote_volume,
            num_trades,
            taker_buy_base_volume,
            taker_buy_quote_volume,
        });
    }

    Ok(BinanceKlines {
        price_exponent,
        qty_exponent,
        klines,
    })
}

/// Block length for new order full response.
const NEW_ORDER_FULL_BLOCK_LENGTH: usize = 153;

/// Block length for cancel order response.
const CANCEL_ORDER_BLOCK_LENGTH: usize = 137;

/// Block length for order response (query).
const ORDER_BLOCK_LENGTH: usize = 153;

/// Decode a new order full response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_new_order_full(buf: &[u8]) -> Result<BinanceNewOrderResponse, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != NEW_ORDER_FULL_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    cursor.require(NEW_ORDER_FULL_BLOCK_LENGTH)?;

    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;
    let order_id = cursor.read_i64_le()?;
    let order_list_id = cursor.read_optional_i64_le()?;
    let transact_time = cursor.read_i64_le()?;
    let price_mantissa = cursor.read_i64_le()?;
    let orig_qty_mantissa = cursor.read_i64_le()?;
    let executed_qty_mantissa = cursor.read_i64_le()?;
    let cummulative_quote_qty_mantissa = cursor.read_i64_le()?;
    let status = cursor.read_u8()?.into();
    let time_in_force = cursor.read_u8()?.into();
    let order_type = cursor.read_u8()?.into();
    let side = cursor.read_u8()?.into();
    let stop_price_mantissa = cursor.read_optional_i64_le()?;

    cursor.advance(16)?; // Skip trailing_delta (8) + trailing_time (8)
    let working_time = cursor.read_optional_i64_le()?;

    cursor.advance(23)?; // Skip iceberg to used_sor
    let self_trade_prevention_mode = cursor.read_u8()?.into();

    cursor.advance(16)?; // Skip trade_group_id + prevented_quantity
    let _commission_exponent = cursor.read_i8()?;

    cursor.advance(18)?; // Skip to end of fixed block

    let fills = decode_fills_cursor(&mut cursor)?;

    // Skip prevented matches group
    let (block_len, count) = cursor.read_group_header()?;
    cursor.advance(block_len as usize * count as usize)?;

    let symbol = cursor.read_var_string8()?;
    let client_order_id = cursor.read_var_string8()?;

    Ok(BinanceNewOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        transact_time,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        stop_price_mantissa,
        working_time,
        self_trade_prevention_mode,
        client_order_id,
        symbol,
        fills,
    })
}

/// Decode a cancel order response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_cancel_order(buf: &[u8]) -> Result<BinanceCancelOrderResponse, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != CANCEL_ORDER_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    cursor.require(CANCEL_ORDER_BLOCK_LENGTH)?;

    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;
    let order_id = cursor.read_i64_le()?;
    let order_list_id = cursor.read_optional_i64_le()?;
    let transact_time = cursor.read_i64_le()?;
    let price_mantissa = cursor.read_i64_le()?;
    let orig_qty_mantissa = cursor.read_i64_le()?;
    let executed_qty_mantissa = cursor.read_i64_le()?;
    let cummulative_quote_qty_mantissa = cursor.read_i64_le()?;
    let status = cursor.read_u8()?.into();
    let time_in_force = cursor.read_u8()?.into();
    let order_type = cursor.read_u8()?.into();
    let side = cursor.read_u8()?.into();
    let self_trade_prevention_mode = cursor.read_u8()?.into();

    cursor.advance(CANCEL_ORDER_BLOCK_LENGTH - 63)?; // Skip to end of fixed block

    let symbol = cursor.read_var_string8()?;
    let orig_client_order_id = cursor.read_var_string8()?;
    let client_order_id = cursor.read_var_string8()?;

    Ok(BinanceCancelOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        transact_time,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        self_trade_prevention_mode,
        client_order_id,
        orig_client_order_id,
        symbol,
    })
}

/// Decode an order query response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_order(buf: &[u8]) -> Result<BinanceOrderResponse, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != ORDER_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    cursor.require(ORDER_BLOCK_LENGTH)?;

    let price_exponent = cursor.read_i8()?;
    let qty_exponent = cursor.read_i8()?;
    let order_id = cursor.read_i64_le()?;
    let order_list_id = cursor.read_optional_i64_le()?;
    let price_mantissa = cursor.read_i64_le()?;
    let orig_qty_mantissa = cursor.read_i64_le()?;
    let executed_qty_mantissa = cursor.read_i64_le()?;
    let cummulative_quote_qty_mantissa = cursor.read_i64_le()?;
    let status = cursor.read_u8()?.into();
    let time_in_force = cursor.read_u8()?.into();
    let order_type = cursor.read_u8()?.into();
    let side = cursor.read_u8()?.into();
    let stop_price_mantissa = cursor.read_optional_i64_le()?;
    let iceberg_qty_mantissa = cursor.read_optional_i64_le()?;
    let time = cursor.read_i64_le()?;
    let update_time = cursor.read_i64_le()?;
    let is_working = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    let working_time = cursor.read_optional_i64_le()?;
    let orig_quote_order_qty_mantissa = cursor.read_i64_le()?;
    let self_trade_prevention_mode = cursor.read_u8()?.into();

    cursor.advance(ORDER_BLOCK_LENGTH - 104)?; // Skip to end of fixed block

    let symbol = cursor.read_var_string8()?;
    let client_order_id = cursor.read_var_string8()?;

    Ok(BinanceOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        stop_price_mantissa,
        iceberg_qty_mantissa,
        time,
        update_time,
        is_working,
        working_time,
        orig_quote_order_qty_mantissa,
        self_trade_prevention_mode,
        client_order_id,
        symbol,
    })
}

/// Block length for orders group item.
const ORDERS_GROUP_BLOCK_LENGTH: usize = 162;

/// Decode multiple orders response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_orders(buf: &[u8]) -> Result<Vec<BinanceOrderResponse>, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != ORDERS_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let (block_length, count) = cursor.read_group_header()?;

    if count == 0 {
        return Ok(Vec::new());
    }

    if block_length as usize != ORDERS_GROUP_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: ORDERS_GROUP_BLOCK_LENGTH as u16,
            actual: block_length,
        });
    }

    let mut orders = Vec::with_capacity(count as usize);

    for _ in 0..count {
        cursor.require(ORDERS_GROUP_BLOCK_LENGTH)?;

        let price_exponent = cursor.read_i8()?;
        let qty_exponent = cursor.read_i8()?;
        let order_id = cursor.read_i64_le()?;
        let order_list_id = cursor.read_optional_i64_le()?;
        let price_mantissa = cursor.read_i64_le()?;
        let orig_qty_mantissa = cursor.read_i64_le()?;
        let executed_qty_mantissa = cursor.read_i64_le()?;
        let cummulative_quote_qty_mantissa = cursor.read_i64_le()?;
        let status = cursor.read_u8()?.into();
        let time_in_force = cursor.read_u8()?.into();
        let order_type = cursor.read_u8()?.into();
        let side = cursor.read_u8()?.into();
        let stop_price_mantissa = cursor.read_optional_i64_le()?;

        cursor.advance(16)?; // Skip trailing_delta + trailing_time
        let iceberg_qty_mantissa = cursor.read_optional_i64_le()?;
        let time = cursor.read_i64_le()?;
        let update_time = cursor.read_i64_le()?;
        let is_working = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
        let working_time = cursor.read_optional_i64_le()?;
        let orig_quote_order_qty_mantissa = cursor.read_i64_le()?;

        cursor.advance(14)?; // Skip strategy_id to working_floor
        let self_trade_prevention_mode = cursor.read_u8()?.into();

        cursor.advance(28)?; // Skip to end of fixed block

        let symbol = cursor.read_var_string8()?;
        let client_order_id = cursor.read_var_string8()?;

        orders.push(BinanceOrderResponse {
            price_exponent,
            qty_exponent,
            order_id,
            order_list_id,
            price_mantissa,
            orig_qty_mantissa,
            executed_qty_mantissa,
            cummulative_quote_qty_mantissa,
            status,
            time_in_force,
            order_type,
            side,
            stop_price_mantissa,
            iceberg_qty_mantissa,
            time,
            update_time,
            is_working,
            working_time,
            orig_quote_order_qty_mantissa,
            self_trade_prevention_mode,
            client_order_id,
            symbol,
        });
    }

    Ok(orders)
}

/// Decode cancel open orders response.
///
/// Each item in the response group contains an embedded cancel_order_response SBE message.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_cancel_open_orders(
    buf: &[u8],
) -> Result<Vec<BinanceCancelOrderResponse>, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != CANCEL_OPEN_ORDERS_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let (_block_length, count) = cursor.read_group_header()?;

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut responses = Vec::with_capacity(count as usize);

    // Each group item has block_length=0, followed by u16 length + embedded SBE message
    for _ in 0..count {
        let response_len = cursor.read_u16_le()? as usize;
        let embedded_bytes = cursor.read_bytes(response_len)?;
        let cancel_response = decode_cancel_order(embedded_bytes)?;
        responses.push(cancel_response);
    }

    Ok(responses)
}

/// Account response block length (from SBE codec).
const ACCOUNT_BLOCK_LENGTH: usize = 64;

/// Balance group item block length (from SBE codec).
const BALANCE_BLOCK_LENGTH: u16 = 17;

/// Decode account information response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_account(buf: &[u8]) -> Result<BinanceAccountInfo, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != ACCOUNT_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    cursor.require(ACCOUNT_BLOCK_LENGTH)?;

    let commission_exponent = cursor.read_i8()?;
    let maker_commission_mantissa = cursor.read_i64_le()?;
    let taker_commission_mantissa = cursor.read_i64_le()?;
    let buyer_commission_mantissa = cursor.read_i64_le()?;
    let seller_commission_mantissa = cursor.read_i64_le()?;
    let can_trade = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    let can_withdraw = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    let can_deposit = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    cursor.advance(1)?; // Skip brokered
    let require_self_trade_prevention = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    let prevent_sor = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
    let update_time = cursor.read_i64_le()?;
    let account_type_enum = AccountType::from(cursor.read_u8()?);
    cursor.advance(16)?; // Skip tradeGroupId + uid

    let account_type = account_type_enum.to_string();

    let (block_length, balance_count) = cursor.read_group_header()?;

    if block_length != BALANCE_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: BALANCE_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut balances = Vec::with_capacity(balance_count as usize);

    for _ in 0..balance_count {
        cursor.require(block_length as usize)?;

        let exponent = cursor.read_i8()?;
        let free_mantissa = cursor.read_i64_le()?;
        let locked_mantissa = cursor.read_i64_le()?;

        let asset = cursor.read_var_string8()?;

        balances.push(BinanceBalance {
            asset,
            free_mantissa,
            locked_mantissa,
            exponent,
        });
    }

    Ok(BinanceAccountInfo {
        commission_exponent,
        maker_commission_mantissa,
        taker_commission_mantissa,
        buyer_commission_mantissa,
        seller_commission_mantissa,
        can_trade,
        can_withdraw,
        can_deposit,
        require_self_trade_prevention,
        prevent_sor,
        update_time,
        account_type,
        balances,
    })
}

/// Account trade group item block length (from SBE codec).
const ACCOUNT_TRADE_BLOCK_LENGTH: u16 = 70;

/// Decode account trades response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
#[allow(dead_code)]
pub fn decode_account_trades(buf: &[u8]) -> Result<Vec<BinanceAccountTrade>, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != ACCOUNT_TRADES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let (block_length, trade_count) = cursor.read_group_header()?;

    if block_length != ACCOUNT_TRADE_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: ACCOUNT_TRADE_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut trades = Vec::with_capacity(trade_count as usize);

    for _ in 0..trade_count {
        cursor.require(block_length as usize)?;

        let price_exponent = cursor.read_i8()?;
        let qty_exponent = cursor.read_i8()?;
        let commission_exponent = cursor.read_i8()?;
        let id = cursor.read_i64_le()?;
        let order_id = cursor.read_i64_le()?;
        let order_list_id = cursor.read_optional_i64_le()?;
        let price_mantissa = cursor.read_i64_le()?;
        let qty_mantissa = cursor.read_i64_le()?;
        let quote_qty_mantissa = cursor.read_i64_le()?;
        let commission_mantissa = cursor.read_i64_le()?;
        let time = cursor.read_i64_le()?;
        let is_buyer = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
        let is_maker = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;
        let is_best_match = BoolEnum::from(cursor.read_u8()?) == BoolEnum::True;

        let symbol = cursor.read_var_string8()?;
        let commission_asset = cursor.read_var_string8()?;

        trades.push(BinanceAccountTrade {
            price_exponent,
            qty_exponent,
            commission_exponent,
            id,
            order_id,
            order_list_id,
            price_mantissa,
            qty_mantissa,
            quote_qty_mantissa,
            commission_mantissa,
            time,
            is_buyer,
            is_maker,
            is_best_match,
            symbol,
            commission_asset,
        });
    }

    Ok(trades)
}

/// Fills group item block length (from SBE codec).
const FILLS_BLOCK_LENGTH: u16 = 42;

/// Decode order fills using cursor.
fn decode_fills_cursor(
    cursor: &mut SbeCursor<'_>,
) -> Result<Vec<BinanceOrderFill>, SbeDecodeError> {
    let (block_length, count) = cursor.read_group_header()?;

    if block_length != FILLS_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: FILLS_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut fills = Vec::with_capacity(count as usize);

    for _ in 0..count {
        cursor.require(block_length as usize)?;

        let commission_exponent = cursor.read_i8()?;
        cursor.advance(1)?; // Skip matchType
        let price_mantissa = cursor.read_i64_le()?;
        let qty_mantissa = cursor.read_i64_le()?;
        let commission_mantissa = cursor.read_i64_le()?;
        let trade_id = cursor.read_optional_i64_le()?;
        cursor.advance(8)?; // Skip allocId

        let commission_asset = cursor.read_var_string8()?;

        fills.push(BinanceOrderFill {
            price_mantissa,
            qty_mantissa,
            commission_mantissa,
            commission_exponent,
            commission_asset,
            trade_id,
        });
    }

    Ok(fills)
}

/// Symbols group block length (from SBE codec).
const SYMBOL_BLOCK_LENGTH: usize = 19;

/// Decode exchange info response.
///
/// ExchangeInfo response contains rate limits, exchange filters, symbols, and SOR info.
/// We only decode the symbols array which contains instrument definitions.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or template ID mismatch.
///
/// # Panics
///
/// This function will panic if filter byte slices cannot be converted to fixed-size arrays,
/// which should not occur if the SBE data is well-formed.
pub fn decode_exchange_info(buf: &[u8]) -> Result<BinanceExchangeInfoSbe, SbeDecodeError> {
    let mut cursor = SbeCursor::new(buf);
    let header = MessageHeader::decode_cursor(&mut cursor)?;
    header.validate()?;

    if header.template_id != EXCHANGE_INFO_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Skip rate_limits group
    let (rate_limits_block_len, rate_limits_count) = cursor.read_group_header()?;
    cursor.advance(rate_limits_block_len as usize * rate_limits_count as usize)?;

    // Skip exchange_filters group
    let (_exchange_filters_block_len, exchange_filters_count) = cursor.read_group_header()?;
    for _ in 0..exchange_filters_count {
        // Each filter is a varString8
        cursor.read_var_string8()?;
    }

    // Decode symbols group
    let (symbols_block_len, symbols_count) = cursor.read_group_header()?;

    if symbols_block_len != SYMBOL_BLOCK_LENGTH as u16 {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: SYMBOL_BLOCK_LENGTH as u16,
            actual: symbols_block_len,
        });
    }

    let mut symbols = Vec::with_capacity(symbols_count as usize);

    for _ in 0..symbols_count {
        cursor.require(SYMBOL_BLOCK_LENGTH)?;

        // Fixed fields (19 bytes)
        let status = cursor.read_u8()?;
        let base_asset_precision = cursor.read_u8()?;
        let quote_asset_precision = cursor.read_u8()?;
        let _base_commission_precision = cursor.read_u8()?;
        let _quote_commission_precision = cursor.read_u8()?;
        let order_types = cursor.read_u16_le()?;
        let iceberg_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let oco_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let oto_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let quote_order_qty_market_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let allow_trailing_stop = cursor.read_u8()? == BoolEnum::True as u8;
        let cancel_replace_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let amend_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let is_spot_trading_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let is_margin_trading_allowed = cursor.read_u8()? == BoolEnum::True as u8;
        let _default_self_trade_prevention_mode = cursor.read_u8()?;
        let _allowed_self_trade_prevention_modes = cursor.read_u8()?;
        let _peg_instructions_allowed = cursor.read_u8()?;

        let (_filters_block_len, filters_count) = cursor.read_group_header()?;
        let mut filters = BinanceSymbolFiltersSbe::default();

        for _ in 0..filters_count {
            let filter_bytes = cursor.read_var_bytes8()?;

            // Filters can have header (8 bytes) or be raw body only,
            // detect format by checking if bytes [2..4] contain a valid template_id
            let (template_id, offset) = if filter_bytes.len() >= HEADER_LENGTH + 2 {
                let potential_template = u16::from_le_bytes([filter_bytes[2], filter_bytes[3]]);
                if potential_template == PRICE_FILTER_TEMPLATE_ID
                    || potential_template == LOT_SIZE_FILTER_TEMPLATE_ID
                {
                    (potential_template, HEADER_LENGTH)
                } else {
                    let raw_template = u16::from_le_bytes([filter_bytes[0], filter_bytes[1]]);
                    (raw_template, 2)
                }
            } else if filter_bytes.len() >= 2 {
                let raw_template = u16::from_le_bytes([filter_bytes[0], filter_bytes[1]]);
                (raw_template, 2)
            } else {
                continue;
            };

            // Filter body layout: exponent(1) + min(8) + max(8) + size(8) = 25 bytes
            match template_id {
                PRICE_FILTER_TEMPLATE_ID => {
                    if filter_bytes.len() >= offset + 25 {
                        let price_exp = filter_bytes[offset] as i8;
                        let min_price = i64::from_le_bytes(
                            filter_bytes[offset + 1..offset + 9].try_into().unwrap(),
                        );
                        let max_price = i64::from_le_bytes(
                            filter_bytes[offset + 9..offset + 17].try_into().unwrap(),
                        );
                        let tick_size = i64::from_le_bytes(
                            filter_bytes[offset + 17..offset + 25].try_into().unwrap(),
                        );
                        filters.price_filter = Some(BinancePriceFilterSbe {
                            price_exponent: price_exp,
                            min_price,
                            max_price,
                            tick_size,
                        });
                    }
                }
                LOT_SIZE_FILTER_TEMPLATE_ID => {
                    if filter_bytes.len() >= offset + 25 {
                        let qty_exp = filter_bytes[offset] as i8;
                        let min_qty = i64::from_le_bytes(
                            filter_bytes[offset + 1..offset + 9].try_into().unwrap(),
                        );
                        let max_qty = i64::from_le_bytes(
                            filter_bytes[offset + 9..offset + 17].try_into().unwrap(),
                        );
                        let step_size = i64::from_le_bytes(
                            filter_bytes[offset + 17..offset + 25].try_into().unwrap(),
                        );
                        filters.lot_size_filter = Some(BinanceLotSizeFilterSbe {
                            qty_exponent: qty_exp,
                            min_qty,
                            max_qty,
                            step_size,
                        });
                    }
                }
                _ => {}
            }
        }

        // Permission sets nested group
        let (_perm_sets_block_len, perm_sets_count) = cursor.read_group_header()?;
        let mut permissions = Vec::with_capacity(perm_sets_count as usize);
        for _ in 0..perm_sets_count {
            // Permissions nested group
            let (_perms_block_len, perms_count) = cursor.read_group_header()?;
            let mut perm_set = Vec::with_capacity(perms_count as usize);
            for _ in 0..perms_count {
                let perm = cursor.read_var_string8()?;
                perm_set.push(perm);
            }
            permissions.push(perm_set);
        }

        // Variable-length strings
        let symbol = cursor.read_var_string8()?;
        let base_asset = cursor.read_var_string8()?;
        let quote_asset = cursor.read_var_string8()?;

        symbols.push(BinanceSymbolSbe {
            symbol,
            base_asset,
            quote_asset,
            base_asset_precision,
            quote_asset_precision,
            status,
            order_types,
            iceberg_allowed,
            oco_allowed,
            oto_allowed,
            quote_order_qty_market_allowed,
            allow_trailing_stop,
            cancel_replace_allowed,
            amend_allowed,
            is_spot_trading_allowed,
            is_margin_trading_allowed,
            filters,
            permissions,
        });
    }

    // Skip SOR group (we don't need it)

    Ok(BinanceExchangeInfoSbe { symbols })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_header(block_length: u16, template_id: u16, schema_id: u16, version: u16) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..4].copy_from_slice(&template_id.to_le_bytes());
        buf[4..6].copy_from_slice(&schema_id.to_le_bytes());
        buf[6..8].copy_from_slice(&version.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_ping_valid() {
        // Ping: block_length=0, template_id=101, schema_id=3, version=1
        let buf = create_header(0, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        assert!(decode_ping(&buf).is_ok());
    }

    #[rstest]
    fn test_decode_ping_buffer_too_short() {
        let buf = [0u8; 4];
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_ping_schema_mismatch() {
        let buf = create_header(0, PING_TEMPLATE_ID, 99, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::SchemaMismatch { .. }));
    }

    #[rstest]
    fn test_decode_ping_wrong_template() {
        let buf = create_header(0, 999, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(999)));
    }

    #[rstest]
    fn test_decode_server_time_valid() {
        // ServerTime: block_length=8, template_id=102, schema_id=3, version=1
        let header = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let timestamp: i64 = 1734300000000; // Example timestamp

        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&timestamp.to_le_bytes());

        let result = decode_server_time(&buf).unwrap();
        assert_eq!(result, timestamp);
    }

    #[rstest]
    fn test_decode_server_time_buffer_too_short() {
        // Header only, missing body
        let buf = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_server_time_wrong_template() {
        let header = create_header(8, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_server_time_version_mismatch() {
        let header = create_header(8, SERVER_TIME_TEMPLATE_ID, SBE_SCHEMA_ID, 99);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::VersionMismatch { .. }));
    }

    fn create_group_header(block_length: u16, count: u32) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..6].copy_from_slice(&count.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_depth_valid() {
        // Depth: block_length=10, template_id=200
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: last_update_id (8) + price_exponent (1) + qty_exponent (1)
        let last_update_id: i64 = 123456789;
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.extend_from_slice(&last_update_id.to_le_bytes());
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Bids group: 2 levels
        buf.extend_from_slice(&create_group_header(16, 2));
        // Bid 1: price=100000000000, qty=50000000
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes());
        buf.extend_from_slice(&50_000_000i64.to_le_bytes());
        // Bid 2: price=99900000000, qty=30000000
        buf.extend_from_slice(&99_900_000_000i64.to_le_bytes());
        buf.extend_from_slice(&30_000_000i64.to_le_bytes());

        // Asks group: 1 level
        buf.extend_from_slice(&create_group_header(16, 1));
        // Ask 1: price=100100000000, qty=25000000
        buf.extend_from_slice(&100_100_000_000i64.to_le_bytes());
        buf.extend_from_slice(&25_000_000i64.to_le_bytes());

        let depth = decode_depth(&buf).unwrap();

        assert_eq!(depth.last_update_id, 123456789);
        assert_eq!(depth.price_exponent, -8);
        assert_eq!(depth.qty_exponent, -8);
        assert_eq!(depth.bids.len(), 2);
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.bids[0].price_mantissa, 100_000_000_000);
        assert_eq!(depth.bids[0].qty_mantissa, 50_000_000);
        assert_eq!(depth.asks[0].price_mantissa, 100_100_000_000);
    }

    #[rstest]
    fn test_decode_depth_empty_book() {
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes()); // last_update_id
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty bids
        buf.extend_from_slice(&create_group_header(16, 0));
        // Empty asks
        buf.extend_from_slice(&create_group_header(16, 0));

        let depth = decode_depth(&buf).unwrap();

        assert!(depth.bids.is_empty());
        assert!(depth.asks.is_empty());
    }

    #[rstest]
    fn test_decode_trades_valid() {
        // Trades: block_length=2, template_id=201
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: price_exponent (1) + qty_exponent (1)
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Trades group: 1 trade (42 bytes each)
        buf.extend_from_slice(&create_group_header(42, 1));

        // Trade: id(8) + price(8) + qty(8) + quoteQty(8) + time(8) + isBuyerMaker(1) + isBestMatch(1)
        let trade_id: i64 = 999;
        let price: i64 = 100_000_000_000;
        let qty: i64 = 10_000_000;
        let quote_qty: i64 = 1_000_000_000_000;
        let time: i64 = 1734300000000;
        let is_buyer_maker: u8 = 1; // true
        let is_best_match: u8 = 1; // true

        buf.extend_from_slice(&trade_id.to_le_bytes());
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
        buf.extend_from_slice(&quote_qty.to_le_bytes());
        buf.extend_from_slice(&time.to_le_bytes());
        buf.push(is_buyer_maker);
        buf.push(is_best_match);

        let trades = decode_trades(&buf).unwrap();

        assert_eq!(trades.price_exponent, -8);
        assert_eq!(trades.qty_exponent, -8);
        assert_eq!(trades.trades.len(), 1);
        assert_eq!(trades.trades[0].id, 999);
        assert_eq!(trades.trades[0].price_mantissa, 100_000_000_000);
        assert!(trades.trades[0].is_buyer_maker);
        assert!(trades.trades[0].is_best_match);
    }

    #[rstest]
    fn test_decode_trades_empty() {
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty trades group
        buf.extend_from_slice(&create_group_header(42, 0));

        let trades = decode_trades(&buf).unwrap();

        assert!(trades.trades.is_empty());
    }

    #[rstest]
    fn test_decode_depth_wrong_template() {
        let header = create_header(10, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 10]); // dummy block

        let err = decode_depth(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_trades_wrong_template() {
        let header = create_header(2, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 2]); // dummy block

        let err = decode_trades(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    fn write_var_string(buf: &mut Vec<u8>, s: &str) {
        buf.push(s.len() as u8);
        buf.extend_from_slice(s.as_bytes());
    }

    #[rstest]
    fn test_decode_order_valid() {
        let header = create_header(
            ORDER_BLOCK_LENGTH as u16,
            ORDER_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Fixed block (153 bytes)
        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&12345i64.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes()); // price_mantissa
        buf.extend_from_slice(&10_000_000i64.to_le_bytes()); // orig_qty_mantissa
        buf.extend_from_slice(&5_000_000i64.to_le_bytes()); // executed_qty_mantissa
        buf.extend_from_slice(&500_000_000i64.to_le_bytes()); // cummulative_quote_qty_mantissa
        buf.push(1); // status (NEW)
        buf.push(1); // time_in_force (GTC)
        buf.push(1); // order_type (LIMIT)
        buf.push(1); // side (BUY)
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // iceberg_qty (None)
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.extend_from_slice(&1734300001000i64.to_le_bytes()); // update_time
        buf.push(1); // is_working (true)
        buf.extend_from_slice(&1734300000500i64.to_le_bytes()); // working_time
        buf.extend_from_slice(&0i64.to_le_bytes()); // orig_quote_order_qty_mantissa
        buf.push(0); // self_trade_prevention_mode

        // Pad to 153 bytes
        while buf.len() < 8 + ORDER_BLOCK_LENGTH {
            buf.push(0);
        }

        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "my-order-123");

        let order = decode_order(&buf).unwrap();

        assert_eq!(order.order_id, 12345);
        assert!(order.order_list_id.is_none());
        assert_eq!(order.price_exponent, -8);
        assert_eq!(order.price_mantissa, 100_000_000_000);
        assert!(order.stop_price_mantissa.is_none());
        assert!(order.iceberg_qty_mantissa.is_none());
        assert!(order.is_working);
        assert_eq!(order.working_time, Some(1734300000500));
        assert_eq!(order.symbol, "BTCUSDT");
        assert_eq!(order.client_order_id, "my-order-123");
    }

    #[rstest]
    fn test_decode_orders_multiple() {
        // This test verifies cursor advances correctly through multiple orders
        let header = create_header(0, ORDERS_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Group header: block_length=162, count=2
        buf.extend_from_slice(&create_group_header(ORDERS_GROUP_BLOCK_LENGTH as u16, 2));

        // Order 1
        let order1_start = buf.len();
        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&1001i64.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes()); // price_mantissa
        buf.extend_from_slice(&10_000_000i64.to_le_bytes()); // orig_qty
        buf.extend_from_slice(&0i64.to_le_bytes()); // executed_qty
        buf.extend_from_slice(&0i64.to_le_bytes()); // cummulative_quote_qty
        buf.push(1); // status
        buf.push(1); // time_in_force
        buf.push(1); // order_type
        buf.push(1); // side
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
        buf.extend_from_slice(&[0u8; 16]); // trailing_delta + trailing_time
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // iceberg_qty (None)
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // update_time
        buf.push(1); // is_working
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // working_time
        buf.extend_from_slice(&0i64.to_le_bytes()); // orig_quote_order_qty

        // Pad to 162 bytes from order start
        while buf.len() - order1_start < ORDERS_GROUP_BLOCK_LENGTH {
            buf.push(0);
        }
        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "order-1");

        // Order 2
        let order2_start = buf.len();
        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&2002i64.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&200_000_000_000i64.to_le_bytes()); // price_mantissa
        buf.extend_from_slice(&20_000_000i64.to_le_bytes()); // orig_qty
        buf.extend_from_slice(&0i64.to_le_bytes()); // executed_qty
        buf.extend_from_slice(&0i64.to_le_bytes()); // cummulative_quote_qty
        buf.push(1); // status
        buf.push(1); // time_in_force
        buf.push(1); // order_type
        buf.push(2); // side (SELL)
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
        buf.extend_from_slice(&[0u8; 16]); // trailing_delta + trailing_time
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // iceberg_qty (None)
        buf.extend_from_slice(&1734300001000i64.to_le_bytes()); // time
        buf.extend_from_slice(&1734300001000i64.to_le_bytes()); // update_time
        buf.push(1); // is_working
        buf.extend_from_slice(&1734300001000i64.to_le_bytes()); // working_time
        buf.extend_from_slice(&0i64.to_le_bytes()); // orig_quote_order_qty

        while buf.len() - order2_start < ORDERS_GROUP_BLOCK_LENGTH {
            buf.push(0);
        }
        write_var_string(&mut buf, "ETHUSDT");
        write_var_string(&mut buf, "order-2");

        let orders = decode_orders(&buf).unwrap();

        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].order_id, 1001);
        assert_eq!(orders[0].symbol, "BTCUSDT");
        assert_eq!(orders[0].client_order_id, "order-1");
        assert_eq!(orders[0].price_mantissa, 100_000_000_000);

        assert_eq!(orders[1].order_id, 2002);
        assert_eq!(orders[1].symbol, "ETHUSDT");
        assert_eq!(orders[1].client_order_id, "order-2");
        assert_eq!(orders[1].price_mantissa, 200_000_000_000);
    }

    #[rstest]
    fn test_decode_orders_empty() {
        let header = create_header(0, ORDERS_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&create_group_header(ORDERS_GROUP_BLOCK_LENGTH as u16, 0));

        let orders = decode_orders(&buf).unwrap();
        assert!(orders.is_empty());
    }

    #[rstest]
    fn test_decode_orders_truncated_var_string() {
        let header = create_header(0, ORDERS_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&create_group_header(ORDERS_GROUP_BLOCK_LENGTH as u16, 1));

        // Pad fixed block to 162 bytes
        buf.extend_from_slice(&[0u8; ORDERS_GROUP_BLOCK_LENGTH]);

        // Symbol length says 7 bytes but we only provide 3
        buf.push(7); // Length prefix claims "BTCUSDT" (7 chars)
        buf.extend_from_slice(b"BTC"); // Only 3 bytes - truncated

        let err = decode_orders(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_orders_invalid_utf8() {
        let header = create_header(0, ORDERS_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&create_group_header(ORDERS_GROUP_BLOCK_LENGTH as u16, 1));

        buf.extend_from_slice(&[0u8; ORDERS_GROUP_BLOCK_LENGTH]);

        // Invalid UTF-8 sequence
        buf.push(4);
        buf.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);

        let err = decode_orders(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::InvalidUtf8));
    }

    #[rstest]
    fn test_decode_cancel_order_valid() {
        let header = create_header(
            CANCEL_ORDER_BLOCK_LENGTH as u16,
            CANCEL_ORDER_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&99999i64.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // transact_time
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes()); // price_mantissa
        buf.extend_from_slice(&10_000_000i64.to_le_bytes()); // orig_qty
        buf.extend_from_slice(&10_000_000i64.to_le_bytes()); // executed_qty
        buf.extend_from_slice(&1_000_000_000i64.to_le_bytes()); // cummulative_quote_qty
        buf.push(4); // status (CANCELED)
        buf.push(1); // time_in_force
        buf.push(1); // order_type
        buf.push(1); // side
        buf.push(0); // self_trade_prevention_mode

        // Pad to block length
        while buf.len() < 8 + CANCEL_ORDER_BLOCK_LENGTH {
            buf.push(0);
        }

        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "orig-client-id");
        write_var_string(&mut buf, "new-client-id");

        let cancel = decode_cancel_order(&buf).unwrap();

        assert_eq!(cancel.order_id, 99999);
        assert!(cancel.order_list_id.is_none());
        assert_eq!(cancel.symbol, "BTCUSDT");
        assert_eq!(cancel.orig_client_order_id, "orig-client-id");
        assert_eq!(cancel.client_order_id, "new-client-id");
    }

    #[rstest]
    fn test_decode_account_with_balances() {
        let header = create_header(
            ACCOUNT_BLOCK_LENGTH as u16,
            ACCOUNT_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Fixed block (64 bytes)
        buf.push((-8i8) as u8); // commission_exponent
        buf.extend_from_slice(&100_000i64.to_le_bytes()); // maker_commission
        buf.extend_from_slice(&100_000i64.to_le_bytes()); // taker_commission
        buf.extend_from_slice(&0i64.to_le_bytes()); // buyer_commission
        buf.extend_from_slice(&0i64.to_le_bytes()); // seller_commission
        buf.push(1); // can_trade
        buf.push(1); // can_withdraw
        buf.push(1); // can_deposit
        buf.push(0); // brokered
        buf.push(0); // require_self_trade_prevention
        buf.push(0); // prevent_sor
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // update_time
        buf.push(1); // account_type (SPOT)

        // Pad to 64 bytes
        while buf.len() < 8 + ACCOUNT_BLOCK_LENGTH {
            buf.push(0);
        }

        // Balances group: 2 balances
        buf.extend_from_slice(&create_group_header(BALANCE_BLOCK_LENGTH, 2));

        // Balance 1: BTC
        buf.push((-8i8) as u8); // exponent
        buf.extend_from_slice(&100_000_000i64.to_le_bytes()); // free (1.0 BTC)
        buf.extend_from_slice(&50_000_000i64.to_le_bytes()); // locked (0.5 BTC)
        write_var_string(&mut buf, "BTC");

        // Balance 2: USDT
        buf.push((-8i8) as u8); // exponent
        buf.extend_from_slice(&1_000_000_000_000i64.to_le_bytes()); // free (10000 USDT)
        buf.extend_from_slice(&0i64.to_le_bytes()); // locked
        write_var_string(&mut buf, "USDT");

        let account = decode_account(&buf).unwrap();

        assert!(account.can_trade);
        assert!(account.can_withdraw);
        assert!(account.can_deposit);
        assert_eq!(account.balances.len(), 2);
        assert_eq!(account.balances[0].asset, "BTC");
        assert_eq!(account.balances[0].free_mantissa, 100_000_000);
        assert_eq!(account.balances[0].locked_mantissa, 50_000_000);
        assert_eq!(account.balances[1].asset, "USDT");
        assert_eq!(account.balances[1].free_mantissa, 1_000_000_000_000);
    }

    #[rstest]
    fn test_decode_account_empty_balances() {
        let header = create_header(
            ACCOUNT_BLOCK_LENGTH as u16,
            ACCOUNT_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Minimal fixed block
        buf.push((-8i8) as u8);
        buf.extend_from_slice(&[0u8; 63]); // Rest of fixed block

        // Empty balances group
        buf.extend_from_slice(&create_group_header(BALANCE_BLOCK_LENGTH, 0));

        let account = decode_account(&buf).unwrap();
        assert!(account.balances.is_empty());
    }

    #[rstest]
    fn test_decode_account_trades_multiple() {
        let header = create_header(
            0,
            ACCOUNT_TRADES_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Group header: 2 trades
        buf.extend_from_slice(&create_group_header(ACCOUNT_TRADE_BLOCK_LENGTH, 2));

        // Trade 1
        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.push((-8i8) as u8); // commission_exponent
        buf.extend_from_slice(&1001i64.to_le_bytes()); // id
        buf.extend_from_slice(&5001i64.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes()); // price
        buf.extend_from_slice(&10_000_000i64.to_le_bytes()); // qty
        buf.extend_from_slice(&1_000_000_000_000i64.to_le_bytes()); // quote_qty
        buf.extend_from_slice(&100_000i64.to_le_bytes()); // commission
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.push(1); // is_buyer
        buf.push(0); // is_maker
        buf.push(1); // is_best_match
        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "BNB");

        // Trade 2
        buf.push((-8i8) as u8);
        buf.push((-8i8) as u8);
        buf.push((-8i8) as u8);
        buf.extend_from_slice(&1002i64.to_le_bytes());
        buf.extend_from_slice(&5002i64.to_le_bytes());
        buf.extend_from_slice(&i64::MIN.to_le_bytes());
        buf.extend_from_slice(&200_000_000_000i64.to_le_bytes());
        buf.extend_from_slice(&5_000_000i64.to_le_bytes());
        buf.extend_from_slice(&1_000_000_000_000i64.to_le_bytes());
        buf.extend_from_slice(&50_000i64.to_le_bytes());
        buf.extend_from_slice(&1734300001000i64.to_le_bytes());
        buf.push(0); // is_buyer (false = seller)
        buf.push(1); // is_maker
        buf.push(1); // is_best_match
        write_var_string(&mut buf, "ETHUSDT");
        write_var_string(&mut buf, "USDT");

        let trades = decode_account_trades(&buf).unwrap();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].id, 1001);
        assert_eq!(trades[0].order_id, 5001);
        assert!(trades[0].order_list_id.is_none());
        assert_eq!(trades[0].symbol, "BTCUSDT");
        assert_eq!(trades[0].commission_asset, "BNB");
        assert!(trades[0].is_buyer);
        assert!(!trades[0].is_maker);

        assert_eq!(trades[1].id, 1002);
        assert_eq!(trades[1].symbol, "ETHUSDT");
        assert_eq!(trades[1].commission_asset, "USDT");
        assert!(!trades[1].is_buyer);
        assert!(trades[1].is_maker);
    }

    #[rstest]
    fn test_decode_account_trades_empty() {
        let header = create_header(
            0,
            ACCOUNT_TRADES_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&create_group_header(ACCOUNT_TRADE_BLOCK_LENGTH, 0));

        let trades = decode_account_trades(&buf).unwrap();
        assert!(trades.is_empty());
    }

    #[rstest]
    fn test_decode_exchange_info_single_symbol() {
        let header = create_header(
            0,
            EXCHANGE_INFO_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Empty rate_limits group
        buf.extend_from_slice(&create_group_header(11, 0));

        // Empty exchange_filters group
        buf.extend_from_slice(&create_group_header(0, 0));

        // Symbols group: 1 symbol with block_length=19
        buf.extend_from_slice(&create_group_header(SYMBOL_BLOCK_LENGTH as u16, 1));

        // Fixed block (19 bytes)
        buf.push(0); // status (Trading)
        buf.push(8); // base_asset_precision
        buf.push(8); // quote_asset_precision
        buf.push(8); // base_commission_precision
        buf.push(8); // quote_commission_precision
        buf.extend_from_slice(&0b0000_0111u16.to_le_bytes()); // order_types (MARKET|LIMIT|STOP_LOSS)
        buf.push(1); // iceberg_allowed (True)
        buf.push(1); // oco_allowed (True)
        buf.push(0); // oto_allowed (False)
        buf.push(1); // quote_order_qty_market_allowed (True)
        buf.push(1); // allow_trailing_stop (True)
        buf.push(1); // cancel_replace_allowed (True)
        buf.push(0); // amend_allowed (False)
        buf.push(1); // is_spot_trading_allowed (True)
        buf.push(0); // is_margin_trading_allowed (False)
        buf.push(0); // default_self_trade_prevention_mode
        buf.push(0); // allowed_self_trade_prevention_modes
        buf.push(0); // peg_instructions_allowed

        // Filters nested group: 0 filters (SBE binary filters are skipped)
        buf.extend_from_slice(&create_group_header(0, 0));

        // Permission sets nested group: 1 set with 1 permission
        buf.extend_from_slice(&create_group_header(0, 1));
        buf.extend_from_slice(&create_group_header(0, 1));
        write_var_string(&mut buf, "SPOT");

        // Variable-length strings
        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "BTC");
        write_var_string(&mut buf, "USDT");

        let info = decode_exchange_info(&buf).unwrap();

        assert_eq!(info.symbols.len(), 1);
        let symbol = &info.symbols[0];
        assert_eq!(symbol.symbol, "BTCUSDT");
        assert_eq!(symbol.base_asset, "BTC");
        assert_eq!(symbol.quote_asset, "USDT");
        assert_eq!(symbol.base_asset_precision, 8);
        assert_eq!(symbol.quote_asset_precision, 8);
        assert_eq!(symbol.status, 0); // Trading
        assert_eq!(symbol.order_types, 0b0000_0111);
        assert!(symbol.iceberg_allowed);
        assert!(symbol.oco_allowed);
        assert!(!symbol.oto_allowed);
        assert!(symbol.quote_order_qty_market_allowed);
        assert!(symbol.allow_trailing_stop);
        assert!(symbol.cancel_replace_allowed);
        assert!(!symbol.amend_allowed);
        assert!(symbol.is_spot_trading_allowed);
        assert!(!symbol.is_margin_trading_allowed);
        assert!(symbol.filters.price_filter.is_none()); // No filters in test data
        assert!(symbol.filters.lot_size_filter.is_none());
        assert_eq!(symbol.permissions.len(), 1);
        assert_eq!(symbol.permissions[0], vec!["SPOT"]);
    }

    #[rstest]
    fn test_decode_exchange_info_empty() {
        let header = create_header(
            0,
            EXCHANGE_INFO_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Empty rate_limits group
        buf.extend_from_slice(&create_group_header(11, 0));

        // Empty exchange_filters group
        buf.extend_from_slice(&create_group_header(0, 0));

        // Empty symbols group
        buf.extend_from_slice(&create_group_header(SYMBOL_BLOCK_LENGTH as u16, 0));

        let info = decode_exchange_info(&buf).unwrap();
        assert!(info.symbols.is_empty());
    }

    #[rstest]
    fn test_decode_exchange_info_wrong_template() {
        let header = create_header(0, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        let err = decode_exchange_info(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_exchange_info_multiple_symbols() {
        let header = create_header(
            0,
            EXCHANGE_INFO_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Empty rate_limits group
        buf.extend_from_slice(&create_group_header(11, 0));

        // Empty exchange_filters group
        buf.extend_from_slice(&create_group_header(0, 0));

        // Symbols group: 2 symbols
        buf.extend_from_slice(&create_group_header(SYMBOL_BLOCK_LENGTH as u16, 2));

        // Symbol 1: BTCUSDT
        buf.push(0); // status
        buf.push(8); // base_asset_precision
        buf.push(8); // quote_asset_precision
        buf.push(8); // base_commission_precision
        buf.push(8); // quote_commission_precision
        buf.extend_from_slice(&0b0000_0011u16.to_le_bytes()); // order_types
        buf.push(1); // iceberg_allowed
        buf.push(1); // oco_allowed
        buf.push(0); // oto_allowed
        buf.push(1); // quote_order_qty_market_allowed
        buf.push(1); // allow_trailing_stop
        buf.push(1); // cancel_replace_allowed
        buf.push(0); // amend_allowed
        buf.push(1); // is_spot_trading_allowed
        buf.push(0); // is_margin_trading_allowed
        buf.push(0); // default_self_trade_prevention_mode
        buf.push(0); // allowed_self_trade_prevention_modes
        buf.push(0); // peg_instructions_allowed
        buf.extend_from_slice(&create_group_header(0, 0)); // No filters
        buf.extend_from_slice(&create_group_header(0, 0)); // No permission sets
        write_var_string(&mut buf, "BTCUSDT");
        write_var_string(&mut buf, "BTC");
        write_var_string(&mut buf, "USDT");

        // Symbol 2: ETHUSDT
        buf.push(0); // status
        buf.push(8); // base_asset_precision
        buf.push(8); // quote_asset_precision
        buf.push(8); // base_commission_precision
        buf.push(8); // quote_commission_precision
        buf.extend_from_slice(&0b0000_0011u16.to_le_bytes()); // order_types
        buf.push(1); // iceberg_allowed
        buf.push(1); // oco_allowed
        buf.push(0); // oto_allowed
        buf.push(1); // quote_order_qty_market_allowed
        buf.push(1); // allow_trailing_stop
        buf.push(1); // cancel_replace_allowed
        buf.push(0); // amend_allowed
        buf.push(1); // is_spot_trading_allowed
        buf.push(1); // is_margin_trading_allowed
        buf.push(0); // default_self_trade_prevention_mode
        buf.push(0); // allowed_self_trade_prevention_modes
        buf.push(0); // peg_instructions_allowed
        buf.extend_from_slice(&create_group_header(0, 0)); // No filters
        buf.extend_from_slice(&create_group_header(0, 0)); // No permission sets
        write_var_string(&mut buf, "ETHUSDT");
        write_var_string(&mut buf, "ETH");
        write_var_string(&mut buf, "USDT");

        let info = decode_exchange_info(&buf).unwrap();

        assert_eq!(info.symbols.len(), 2);
        assert_eq!(info.symbols[0].symbol, "BTCUSDT");
        assert_eq!(info.symbols[0].base_asset, "BTC");
        assert!(!info.symbols[0].is_margin_trading_allowed);

        assert_eq!(info.symbols[1].symbol, "ETHUSDT");
        assert_eq!(info.symbols[1].base_asset, "ETH");
        assert!(info.symbols[1].is_margin_trading_allowed);
    }
}
