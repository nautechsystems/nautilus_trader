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
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10},
    enums::OrderSide,
};

use super::{
    super::{SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter},
    FromSbeReuse, MarketSbeMessage,
    common::{
        BOOK_ORDER_BLOCK_LENGTH, DEPTH10_COUNTS_BLOCK_LENGTH, DEPTH10_LEVEL_BLOCK_LENGTH,
        DEPTH10_LEVEL_COUNT, GROUP_HEADER_16_LENGTH, ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
        decode_book_action, decode_header, decode_instrument_id, decode_order_side, decode_price,
        decode_quantity, decode_unix_nanos, encode_group_header_16, encode_instrument_id,
        encode_price, encode_quantity, encode_unix_nanos, encoded_instrument_id_size,
        validate_header,
    },
    template_id,
};

impl MarketSbeMessage for BookOrder {
    const TEMPLATE_ID: u16 = template_id::BOOK_ORDER;
    const BLOCK_LENGTH: u16 = BOOK_ORDER_BLOCK_LENGTH;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_book_order(writer, self);
        Ok(())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        decode_book_order(cursor)
    }
}

impl MarketSbeMessage for OrderBookDelta {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DELTA;
    const BLOCK_LENGTH: u16 = ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_order_book_delta_fields(writer, self);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let action = decode_book_action(cursor)?;
        let order = decode_book_order(cursor)?;
        let flags = cursor.read_u8()?;
        let sequence = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for OrderBookDeltas {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DELTAS;
    const BLOCK_LENGTH: u16 = 25;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        writer.write_u8(self.flags);
        writer.write_u64_le(self.sequence);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)?;
        encode_group_header_16(
            writer,
            "OrderBookDeltas.deltas",
            self.deltas.len(),
            ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
        )?;

        for delta in &self.deltas {
            encode_order_book_delta_fields(writer, delta);
            encode_instrument_id(writer, &delta.instrument_id)?;
        }
        Ok(())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let mut scratch = Vec::new();
        decode_order_book_deltas_body(cursor, &mut scratch)
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
            + encoded_instrument_id_size(&self.instrument_id)
            + GROUP_HEADER_16_LENGTH
            + self
                .deltas
                .iter()
                .map(encoded_order_book_delta_size)
                .sum::<usize>()
    }
}

impl FromSbeReuse for OrderBookDeltas {
    type Scratch = Vec<OrderBookDelta>;

    fn from_sbe_reuse(
        bytes: &[u8],
        scratch: &mut Vec<OrderBookDelta>,
    ) -> Result<Self, SbeDecodeError> {
        let mut cursor = SbeCursor::new(bytes);
        let header = decode_header(&mut cursor)?;
        validate_header(
            header,
            <Self as MarketSbeMessage>::TEMPLATE_ID,
            <Self as MarketSbeMessage>::BLOCK_LENGTH,
        )?;
        decode_order_book_deltas_body(&mut cursor, scratch)
    }
}

fn decode_order_book_deltas_body(
    cursor: &mut SbeCursor<'_>,
    scratch: &mut Vec<OrderBookDelta>,
) -> Result<OrderBookDeltas, SbeDecodeError> {
    let flags = cursor.read_u8()?;
    let sequence = cursor.read_u64_le()?;
    let ts_event = decode_unix_nanos(cursor)?;
    let ts_init = decode_unix_nanos(cursor)?;
    let instrument_id = decode_instrument_id(cursor)?;
    let (block_length, count) = cursor.read_group_header_16()?;

    if block_length != ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let count = usize::from(count);
    scratch.clear();
    scratch.reserve(count);

    for _ in 0..count {
        let action = decode_book_action(cursor)?;
        let order = decode_book_order(cursor)?;
        let delta_flags = cursor.read_u8()?;
        let delta_sequence = cursor.read_u64_le()?;
        let delta_ts_event = decode_unix_nanos(cursor)?;
        let delta_ts_init = decode_unix_nanos(cursor)?;
        let delta_instrument_id = decode_instrument_id(cursor)?;

        scratch.push(OrderBookDelta {
            instrument_id: delta_instrument_id,
            action,
            order,
            flags: delta_flags,
            sequence: delta_sequence,
            ts_event: delta_ts_event,
            ts_init: delta_ts_init,
        });
    }

    Ok(OrderBookDeltas {
        instrument_id,
        deltas: std::mem::take(scratch),
        flags,
        sequence,
        ts_event,
        ts_init,
    })
}

impl MarketSbeMessage for OrderBookDepth10 {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DEPTH10;
    const BLOCK_LENGTH: u16 =
        (DEPTH10_LEVEL_BLOCK_LENGTH * 20) + (DEPTH10_COUNTS_BLOCK_LENGTH as u16 * 2) + 25;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        for bid in &self.bids {
            encode_price(writer, &bid.price);
            encode_quantity(writer, &bid.size);
        }

        for ask in &self.asks {
            encode_price(writer, &ask.price);
            encode_quantity(writer, &ask.size);
        }

        for count in &self.bid_counts {
            writer.write_u32_le(*count);
        }

        for count in &self.ask_counts {
            writer.write_u32_le(*count);
        }
        writer.write_u8(self.flags);
        writer.write_u64_le(self.sequence);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let mut bids = [BookOrder::default(); DEPTH10_LEVEL_COUNT];
        let mut asks = [BookOrder::default(); DEPTH10_LEVEL_COUNT];

        for bid in &mut bids {
            *bid = BookOrder::new(
                OrderSide::Buy,
                decode_price(cursor)?,
                decode_quantity(cursor)?,
                0,
            );
        }

        for ask in &mut asks {
            *ask = BookOrder::new(
                OrderSide::Sell,
                decode_price(cursor)?,
                decode_quantity(cursor)?,
                0,
            );
        }

        let mut bid_counts = [0u32; DEPTH10_LEVEL_COUNT];
        let mut ask_counts = [0u32; DEPTH10_LEVEL_COUNT];

        for count in &mut bid_counts {
            *count = cursor.read_u32_le()?;
        }

        for count in &mut ask_counts {
            *count = cursor.read_u32_le()?;
        }

        let flags = cursor.read_u8()?;
        let sequence = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

fn encode_book_order(writer: &mut SbeWriter<'_>, order: &BookOrder) {
    encode_price(writer, &order.price);
    encode_quantity(writer, &order.size);
    writer.write_u8(order.side as u8);
    writer.write_u64_le(order.order_id);
}

fn decode_book_order(cursor: &mut SbeCursor<'_>) -> Result<BookOrder, SbeDecodeError> {
    let price = decode_price(cursor)?;
    let size = decode_quantity(cursor)?;
    let side = decode_order_side(cursor)?;
    let order_id = cursor.read_u64_le()?;
    Ok(BookOrder {
        side,
        price,
        size,
        order_id,
    })
}

fn encode_order_book_delta_fields(writer: &mut SbeWriter<'_>, delta: &OrderBookDelta) {
    writer.write_u8(delta.action as u8);
    encode_book_order(writer, &delta.order);
    writer.write_u8(delta.flags);
    writer.write_u64_le(delta.sequence);
    encode_unix_nanos(writer, delta.ts_event);
    encode_unix_nanos(writer, delta.ts_init);
}

fn encoded_order_book_delta_size(delta: &OrderBookDelta) -> usize {
    usize::from(ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH)
        + encoded_instrument_id_size(&delta.instrument_id)
}
