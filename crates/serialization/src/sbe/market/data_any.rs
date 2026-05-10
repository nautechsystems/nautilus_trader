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

use nautilus_model::data::{
    Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
    OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
};

use super::{
    super::{SbeCursor, SbeDecodeError, SbeEncodeError, SbeWriter},
    DataAny, MarketSbeMessage, data_any_variant, template_id,
};

impl MarketSbeMessage for DataAny {
    const TEMPLATE_ID: u16 = template_id::DATA_ANY;
    const BLOCK_LENGTH: u16 = 2;

    fn encode_body(&self, writer: &mut SbeWriter<'_>) -> Result<(), SbeEncodeError> {
        match self {
            Self::OrderBookDelta(value) => {
                writer.write_u16_le(data_any_variant::ORDER_BOOK_DELTA);
                <OrderBookDelta as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::OrderBookDeltas(value) => {
                writer.write_u16_le(data_any_variant::ORDER_BOOK_DELTAS);
                <OrderBookDeltas as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::OrderBookDepth10(value) => {
                writer.write_u16_le(data_any_variant::ORDER_BOOK_DEPTH10);
                <OrderBookDepth10 as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::Quote(value) => {
                writer.write_u16_le(data_any_variant::QUOTE);
                <QuoteTick as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::Trade(value) => {
                writer.write_u16_le(data_any_variant::TRADE);
                <TradeTick as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::Bar(value) => {
                writer.write_u16_le(data_any_variant::BAR);
                <Bar as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::MarkPrice(value) => {
                writer.write_u16_le(data_any_variant::MARK_PRICE);
                <MarkPriceUpdate as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::IndexPrice(value) => {
                writer.write_u16_le(data_any_variant::INDEX_PRICE);
                <IndexPriceUpdate as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::FundingRate(value) => {
                writer.write_u16_le(data_any_variant::FUNDING_RATE);
                <FundingRateUpdate as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::InstrumentStatus(value) => {
                writer.write_u16_le(data_any_variant::INSTRUMENT_STATUS);
                <InstrumentStatus as MarketSbeMessage>::encode_body(value, writer)
            }
            Self::InstrumentClose(value) => {
                writer.write_u16_le(data_any_variant::INSTRUMENT_CLOSE);
                <InstrumentClose as MarketSbeMessage>::encode_body(value, writer)
            }
        }
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let variant = cursor.read_u16_le()?;

        match variant {
            data_any_variant::ORDER_BOOK_DELTA => Ok(Self::OrderBookDelta(
                <OrderBookDelta as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::ORDER_BOOK_DELTAS => Ok(Self::OrderBookDeltas(
                <OrderBookDeltas as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::ORDER_BOOK_DEPTH10 => Ok(Self::OrderBookDepth10(
                <OrderBookDepth10 as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::QUOTE => Ok(Self::Quote(
                <QuoteTick as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::TRADE => Ok(Self::Trade(
                <TradeTick as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::BAR => Ok(Self::Bar(<Bar as MarketSbeMessage>::decode_body(cursor)?)),
            data_any_variant::MARK_PRICE => Ok(Self::MarkPrice(
                <MarkPriceUpdate as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::INDEX_PRICE => Ok(Self::IndexPrice(
                <IndexPriceUpdate as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::FUNDING_RATE => Ok(Self::FundingRate(
                <FundingRateUpdate as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::INSTRUMENT_STATUS => Ok(Self::InstrumentStatus(
                <InstrumentStatus as MarketSbeMessage>::decode_body(cursor)?,
            )),
            data_any_variant::INSTRUMENT_CLOSE => Ok(Self::InstrumentClose(
                <InstrumentClose as MarketSbeMessage>::decode_body(cursor)?,
            )),
            _ => Err(SbeDecodeError::InvalidEnumValue {
                type_name: "DataAny",
                value: variant,
            }),
        }
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
            + match self {
                Self::OrderBookDelta(value) => value.encoded_body_size(),
                Self::OrderBookDeltas(value) => value.encoded_body_size(),
                Self::OrderBookDepth10(value) => value.encoded_body_size(),
                Self::Quote(value) => value.encoded_body_size(),
                Self::Trade(value) => value.encoded_body_size(),
                Self::Bar(value) => value.encoded_body_size(),
                Self::MarkPrice(value) => value.encoded_body_size(),
                Self::IndexPrice(value) => value.encoded_body_size(),
                Self::FundingRate(value) => value.encoded_body_size(),
                Self::InstrumentStatus(value) => value.encoded_body_size(),
                Self::InstrumentClose(value) => value.encoded_body_size(),
            }
    }
}

impl From<OrderBookDelta> for DataAny {
    fn from(value: OrderBookDelta) -> Self {
        Self::OrderBookDelta(value)
    }
}

impl From<OrderBookDeltas> for DataAny {
    fn from(value: OrderBookDeltas) -> Self {
        Self::OrderBookDeltas(value)
    }
}

impl From<OrderBookDepth10> for DataAny {
    fn from(value: OrderBookDepth10) -> Self {
        Self::OrderBookDepth10(value)
    }
}

impl From<QuoteTick> for DataAny {
    fn from(value: QuoteTick) -> Self {
        Self::Quote(value)
    }
}

impl From<TradeTick> for DataAny {
    fn from(value: TradeTick) -> Self {
        Self::Trade(value)
    }
}

impl From<Bar> for DataAny {
    fn from(value: Bar) -> Self {
        Self::Bar(value)
    }
}

impl From<MarkPriceUpdate> for DataAny {
    fn from(value: MarkPriceUpdate) -> Self {
        Self::MarkPrice(value)
    }
}

impl From<IndexPriceUpdate> for DataAny {
    fn from(value: IndexPriceUpdate) -> Self {
        Self::IndexPrice(value)
    }
}

impl From<FundingRateUpdate> for DataAny {
    fn from(value: FundingRateUpdate) -> Self {
        Self::FundingRate(value)
    }
}

impl From<InstrumentStatus> for DataAny {
    fn from(value: InstrumentStatus) -> Self {
        Self::InstrumentStatus(value)
    }
}

impl From<InstrumentClose> for DataAny {
    fn from(value: InstrumentClose) -> Self {
        Self::InstrumentClose(value)
    }
}
