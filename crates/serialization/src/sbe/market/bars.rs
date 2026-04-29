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

use nautilus_model::data::{Bar, BarSpecification, BarType};

use super::{
    super::{SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter},
    MarketSbeMessage,
    common::{
        BAR_TYPE_BLOCK_LENGTH, PRICE_BLOCK_LENGTH, QUANTITY_BLOCK_LENGTH,
        decode_aggregation_source, decode_bar_aggregation, decode_instrument_id,
        decode_non_zero_step, decode_price, decode_price_type, decode_quantity, decode_unix_nanos,
        encode_instrument_id, encode_price, encode_quantity, encode_unix_nanos,
        encoded_instrument_id_size,
    },
    template_id,
};

impl MarketSbeMessage for BarType {
    const TEMPLATE_ID: u16 = template_id::BAR_TYPE;
    const BLOCK_LENGTH: u16 = BAR_TYPE_BLOCK_LENGTH;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_bar_specification_fields(writer, self.spec())?;
        writer.write_u8(self.aggregation_source() as u8);
        encode_instrument_id(writer, &self.instrument_id())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let spec = decode_bar_specification(cursor)?;
        let aggregation_source = decode_aggregation_source(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;
        Ok(Self::new(instrument_id, spec, aggregation_source))
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id())
    }
}

impl MarketSbeMessage for Bar {
    const TEMPLATE_ID: u16 = template_id::BAR;
    const BLOCK_LENGTH: u16 =
        BAR_TYPE_BLOCK_LENGTH + (PRICE_BLOCK_LENGTH * 4) + QUANTITY_BLOCK_LENGTH + 16;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        encode_bar_specification_fields(writer, self.bar_type.spec())?;
        writer.write_u8(self.bar_type.aggregation_source() as u8);
        encode_price(writer, &self.open);
        encode_price(writer, &self.high);
        encode_price(writer, &self.low);
        encode_price(writer, &self.close);
        encode_quantity(writer, &self.volume);
        encode_unix_nanos(writer, self.ts_event);
        encode_unix_nanos(writer, self.ts_init);
        encode_instrument_id(writer, &self.bar_type.instrument_id())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let spec = decode_bar_specification(cursor)?;
        let aggregation_source = decode_aggregation_source(cursor)?;
        let open = decode_price(cursor)?;
        let high = decode_price(cursor)?;
        let low = decode_price(cursor)?;
        let close = decode_price(cursor)?;
        let volume = decode_quantity(cursor)?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            bar_type: BarType::new(instrument_id, spec, aggregation_source),
            open,
            high,
            low,
            close,
            volume,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.bar_type.instrument_id())
    }
}

fn encode_bar_specification_fields(
    writer: &mut SbeWriter<'_>,
    spec: BarSpecification,
) -> Result<(), SbeEncodeError> {
    let step = u32::try_from(spec.step.get()).map_err(|_| SbeEncodeError::NumericOverflow {
        field: "BarSpecification.step",
    })?;
    writer.write_u32_le(step);
    writer.write_u8(spec.aggregation as u8);
    writer.write_u8(spec.price_type as u8);
    Ok(())
}

fn decode_bar_specification(
    cursor: &mut SbeCursor<'_>,
) -> Result<BarSpecification, SbeDecodeError> {
    let step = decode_non_zero_step(cursor.read_u32_le()?)?;
    let aggregation = decode_bar_aggregation(cursor)?;
    let price_type = decode_price_type(cursor)?;
    Ok(BarSpecification {
        step,
        aggregation,
        price_type,
    })
}
