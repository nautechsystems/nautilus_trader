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

use nautilus_model::{
    data::{
        FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
        QuoteTick, TradeTick,
    },
    identifiers::TradeId,
};

use super::{
    super::{SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter},
    MarketSbeMessage,
    common::{
        DECIMAL_BLOCK_LENGTH, PRICE_BLOCK_LENGTH, QUANTITY_BLOCK_LENGTH, decode_aggressor_side,
        decode_decimal, decode_instrument_close_type, decode_instrument_id,
        decode_market_status_action, decode_optional_bool, decode_optional_ustr, decode_price,
        decode_quantity, decode_unix_nanos, encode_decimal, encode_instrument_id,
        encode_optional_bool, encode_optional_ustr, encode_price, encode_quantity,
        encode_unix_nanos, encode_var_string16, encoded_instrument_id_size,
        encoded_optional_ustr_size, encoded_var_string16_size,
    },
    template_id,
};

impl MarketSbeMessage for QuoteTick {
    const TEMPLATE_ID: u16 = template_id::QUOTE_TICK;
    const BLOCK_LENGTH: u16 = (PRICE_BLOCK_LENGTH * 2) + (QUANTITY_BLOCK_LENGTH * 2) + 16;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_price(writer, &self.bid_price);
        encode_price(writer, &self.ask_price);
        encode_quantity(writer, &self.bid_size);
        encode_quantity(writer, &self.ask_size);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let bid_price = decode_price(cursor)?;
        let ask_price = decode_price(cursor)?;
        let bid_size = decode_quantity(cursor)?;
        let ask_size = decode_quantity(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for TradeTick {
    const TEMPLATE_ID: u16 = template_id::TRADE_TICK;
    const BLOCK_LENGTH: u16 = PRICE_BLOCK_LENGTH + QUANTITY_BLOCK_LENGTH + 17;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_price(writer, &self.price);
        encode_quantity(writer, &self.size);
        writer.write_u8(self.aggressor_side as u8);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)?;
        encode_var_string16(writer, "TradeTick.trade_id", self.trade_id.as_str())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let price = decode_price(cursor)?;
        let size = decode_quantity(cursor)?;
        let aggressor_side = decode_aggressor_side(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;
        let trade_id = TradeId::new(cursor.read_var_string16_ref()?);

        Ok(Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
            + encoded_instrument_id_size(&self.instrument_id)
            + encoded_var_string16_size(self.trade_id.as_str())
    }
}

impl MarketSbeMessage for MarkPriceUpdate {
    const TEMPLATE_ID: u16 = template_id::MARK_PRICE_UPDATE;
    const BLOCK_LENGTH: u16 = PRICE_BLOCK_LENGTH + 16;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_price(writer, &self.value);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let value = decode_price(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            value,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for IndexPriceUpdate {
    const TEMPLATE_ID: u16 = template_id::INDEX_PRICE_UPDATE;
    const BLOCK_LENGTH: u16 = PRICE_BLOCK_LENGTH + 16;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_price(writer, &self.value);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let value = decode_price(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            value,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for FundingRateUpdate {
    const TEMPLATE_ID: u16 = template_id::FUNDING_RATE_UPDATE;
    const BLOCK_LENGTH: u16 = DECIMAL_BLOCK_LENGTH + 26;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_decimal(writer, &self.rate);
        writer.write_u16_le(self.interval.unwrap_or(u16::MAX));
        writer.write_u64_le(self.next_funding_ns.map_or(u64::MAX, |value| *value));
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let rate = decode_decimal(cursor)?;
        let interval_raw = cursor.read_u16_le()?;
        let next_funding_raw = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            rate,
            interval: (interval_raw != u16::MAX).then_some(interval_raw),
            next_funding_ns: (next_funding_raw != u64::MAX).then_some(next_funding_raw.into()),
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for InstrumentClose {
    const TEMPLATE_ID: u16 = template_id::INSTRUMENT_CLOSE;
    const BLOCK_LENGTH: u16 = PRICE_BLOCK_LENGTH + 17;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_price(writer, &self.close_price);
        writer.write_u8(self.close_type as u8);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let close_price = decode_price(cursor)?;
        let close_type = decode_instrument_close_type(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            close_price,
            close_type,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for InstrumentStatus {
    const TEMPLATE_ID: u16 = template_id::INSTRUMENT_STATUS;
    const BLOCK_LENGTH: u16 = 21;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        writer.write_u16_le(self.action as u16);
        writer.write_u8(encode_optional_bool(self.is_trading));
        writer.write_u8(encode_optional_bool(self.is_quoting));
        writer.write_u8(encode_optional_bool(self.is_short_sell_restricted));
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)?;
        encode_optional_ustr(writer, "InstrumentStatus.reason", self.reason)?;
        encode_optional_ustr(writer, "InstrumentStatus.trading_event", self.trading_event)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let action = decode_market_status_action(cursor)?;
        let is_trading = decode_optional_bool(cursor, "InstrumentStatus.is_trading")?;
        let is_quoting = decode_optional_bool(cursor, "InstrumentStatus.is_quoting")?;
        let is_short_sell_restricted =
            decode_optional_bool(cursor, "InstrumentStatus.is_short_sell_restricted")?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;
        let reason = decode_optional_ustr(cursor)?;
        let trading_event = decode_optional_ustr(cursor)?;

        Ok(Self {
            instrument_id,
            action,
            ts_event,
            ts_init,
            reason,
            trading_event,
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
            + encoded_instrument_id_size(&self.instrument_id)
            + encoded_optional_ustr_size(self.reason)
            + encoded_optional_ustr_size(self.trading_event)
    }
}
