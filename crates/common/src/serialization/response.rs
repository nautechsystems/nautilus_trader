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

use std::{borrow::Cow, sync::Arc};

use bytes::Bytes;
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BookOrder, CustomData, Data, DataType},
    enums::{BookType, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    orderbook::OrderBook,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::messages::data::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, BookResponse, CustomDataResponse,
    DataResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
    OptionChainReferencePriceResponse, Payload, QuotesResponse, TradesResponse,
};

impl Serialize for DataResponse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireResponse::from_native(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DataResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        WireResponse::deserialize(deserializer)?
            .into_native()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
#[allow(
    clippy::large_enum_variant,
    reason = "model features vary enum sizes; transient serialization views avoid an extra allocation"
)]
enum WireResponse<'a> {
    Data(WireCustomResponse<'a>),
    Book(WireBookResponse<'a>),
    Instrument(Cow<'a, InstrumentResponse>),
    Instruments(Cow<'a, InstrumentsResponse>),
    BookDeltas(Cow<'a, BookDeltasResponse>),
    BookDepth(Cow<'a, BookDepthResponse>),
    Quotes(Cow<'a, QuotesResponse>),
    Trades(Cow<'a, TradesResponse>),
    FundingRates(Cow<'a, FundingRatesResponse>),
    OptionChainReferencePrice(Cow<'a, OptionChainReferencePriceResponse>),
    Bars(Cow<'a, BarsResponse>),
}

impl<'a> WireResponse<'a> {
    fn from_native(value: &'a DataResponse) -> anyhow::Result<Self> {
        Ok(match value {
            DataResponse::Data(value) => Self::Data(WireCustomResponse::from_native(value)?),
            DataResponse::Book(value) => Self::Book(WireBookResponse::from_native(value)),
            DataResponse::Instrument(value) => Self::Instrument(Cow::Borrowed(value)),
            DataResponse::Instruments(value) => Self::Instruments(Cow::Borrowed(value)),
            DataResponse::BookDeltas(value) => Self::BookDeltas(Cow::Borrowed(value)),
            DataResponse::BookDepth(value) => Self::BookDepth(Cow::Borrowed(value)),
            DataResponse::Quotes(value) => Self::Quotes(Cow::Borrowed(value)),
            DataResponse::Trades(value) => Self::Trades(Cow::Borrowed(value)),
            DataResponse::FundingRates(value) => Self::FundingRates(Cow::Borrowed(value)),
            DataResponse::OptionChainReferencePrice(value) => {
                Self::OptionChainReferencePrice(Cow::Borrowed(value))
            }
            DataResponse::Bars(value) => Self::Bars(Cow::Borrowed(value)),
        })
    }

    fn into_native(self) -> anyhow::Result<DataResponse> {
        Ok(match self {
            Self::Data(value) => DataResponse::Data(value.into_native()?),
            Self::Book(value) => DataResponse::Book(value.into_native()?),
            Self::Instrument(value) => DataResponse::Instrument(Box::new(value.into_owned())),
            Self::Instruments(value) => DataResponse::Instruments(value.into_owned()),
            Self::BookDeltas(value) => DataResponse::BookDeltas(value.into_owned()),
            Self::BookDepth(value) => DataResponse::BookDepth(value.into_owned()),
            Self::Quotes(value) => DataResponse::Quotes(value.into_owned()),
            Self::Trades(value) => DataResponse::Trades(value.into_owned()),
            Self::FundingRates(value) => DataResponse::FundingRates(value.into_owned()),
            Self::OptionChainReferencePrice(value) => {
                DataResponse::OptionChainReferencePrice(value.into_owned())
            }
            Self::Bars(value) => DataResponse::Bars(value.into_owned()),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct WireCustomResponse<'a> {
    correlation_id: UUID4,
    client_id: ClientId,
    venue: Option<Venue>,
    data_type: Cow<'a, DataType>,
    data: CustomPayload<'a>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    ts_init: UnixNanos,
    params: Cow<'a, Option<Params>>,
}

impl<'a> WireCustomResponse<'a> {
    fn from_native(value: &'a CustomDataResponse) -> anyhow::Result<Self> {
        Ok(Self {
            correlation_id: value.correlation_id,
            client_id: value.client_id,
            venue: value.venue,
            data_type: Cow::Borrowed(&value.data_type),
            data: CustomPayload::from_native(value.data.as_ref())?,
            start: value.start,
            end: value.end,
            ts_init: value.ts_init,
            params: Cow::Borrowed(&value.params),
        })
    }

    fn into_native(self) -> anyhow::Result<CustomDataResponse> {
        Ok(CustomDataResponse {
            correlation_id: self.correlation_id,
            client_id: self.client_id,
            venue: self.venue,
            data_type: self.data_type.into_owned(),
            data: self.data.into_native()?,
            start: self.start,
            end: self.end,
            ts_init: self.ts_init,
            params: self.params.into_owned(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "transient serialization views avoid an extra allocation for scalar payloads"
)]
pub(super) enum CustomPayload<'a> {
    Scalar(CustomItem<'a>),
    Batch(Vec<CustomItem<'a>>),
    Bytes(Cow<'a, [u8]>),
    Buffer(Cow<'a, Bytes>),
    Text(Cow<'a, str>),
}

impl<'a> CustomPayload<'a> {
    pub(super) fn from_native(value: &'a dyn std::any::Any) -> anyhow::Result<Self> {
        if let Some(value) = value.downcast_ref::<CustomData>() {
            return Ok(Self::Scalar(CustomItem::from_native(value)));
        }

        if let Some(value) = value.downcast_ref::<Vec<CustomData>>() {
            return Ok(Self::Batch(
                value.iter().map(CustomItem::from_native).collect(),
            ));
        }

        if let Some(value) = value.downcast_ref::<Vec<u8>>() {
            return Ok(Self::Bytes(Cow::Borrowed(value)));
        }

        if let Some(value) = value.downcast_ref::<Bytes>() {
            return Ok(Self::Buffer(Cow::Borrowed(value)));
        }

        if let Some(value) = value.downcast_ref::<String>() {
            return Ok(Self::Text(Cow::Borrowed(value)));
        }

        anyhow::bail!(
            "custom response payload requires CustomData, Vec<CustomData>, Vec<u8>, Bytes, or String"
        )
    }

    pub(super) fn into_native(self) -> anyhow::Result<Payload> {
        Ok(match self {
            Self::Scalar(value) => Arc::new(value.into_native()?),
            Self::Batch(values) => Arc::new(
                values
                    .into_iter()
                    .map(CustomItem::into_native)
                    .collect::<anyhow::Result<Vec<_>>>()?,
            ),
            Self::Bytes(value) => Arc::new(value.into_owned()),
            Self::Buffer(value) => Arc::new(value.into_owned()),
            Self::Text(value) => Arc::new(value.into_owned()),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct CustomItem<'a> {
    pub(super) data_type: Cow<'a, DataType>,
    pub(super) value: CustomValue<'a>,
}

impl<'a> CustomItem<'a> {
    pub(super) fn from_native(value: &'a CustomData) -> Self {
        Self {
            data_type: Cow::Borrowed(&value.data_type),
            value: CustomValue::Borrowed(value),
        }
    }

    fn into_native(self) -> anyhow::Result<CustomData> {
        let mut value = match self.value {
            CustomValue::Borrowed(value) => value.clone(),
            CustomValue::Owned(Data::Custom(value)) => value,
            CustomValue::Owned(_) => {
                anyhow::bail!("custom response contains a non-custom payload")
            }
        };

        value.data_type = self.data_type.into_owned();
        Ok(value)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
#[allow(
    clippy::large_enum_variant,
    reason = "model features vary enum sizes; owned decoding avoids an extra allocation"
)]
pub(super) enum CustomValue<'a> {
    Borrowed(&'a CustomData),
    Owned(Data),
}

impl<'de> Deserialize<'de> for CustomValue<'_> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Data::deserialize(deserializer).map(Self::Owned)
    }
}

#[derive(Serialize, Deserialize)]
struct WireBookResponse<'a> {
    correlation_id: UUID4,
    client_id: ClientId,
    instrument_id: InstrumentId,
    data: BookState,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    ts_init: UnixNanos,
    params: Cow<'a, Option<Params>>,
}

impl<'a> WireBookResponse<'a> {
    fn from_native(value: &'a BookResponse) -> Self {
        let book = &value.data;

        Self {
            correlation_id: value.correlation_id,
            client_id: value.client_id,
            instrument_id: value.instrument_id,
            data: BookState {
                instrument_id: book.instrument_id,
                book_type: book.book_type,
                sequence: book.sequence,
                ts_last: book.ts_last,
                update_count: book.update_count,
                orders: book
                    .bids(None)
                    .chain(book.asks(None))
                    .flat_map(|level| level.iter().copied())
                    .collect(),
                bid_batch_state: book.batch_state_code(OrderSide::Buy),
                ask_batch_state: book.batch_state_code(OrderSide::Sell),
            },
            start: value.start,
            end: value.end,
            ts_init: value.ts_init,
            params: Cow::Borrowed(&value.params),
        }
    }

    fn into_native(self) -> anyhow::Result<BookResponse> {
        let mut book = OrderBook::new(self.data.instrument_id, self.data.book_type);

        for order in self.data.orders {
            anyhow::ensure!(order.side.is_some(), "book snapshot order has no side");
            book.add(
                order,
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
                self.data.sequence,
                self.data.ts_last,
            );
        }

        book.set_batch_state_code(OrderSide::Buy, self.data.bid_batch_state)?;
        book.set_batch_state_code(OrderSide::Sell, self.data.ask_batch_state)?;
        book.sequence = self.data.sequence;
        book.ts_last = self.data.ts_last;
        book.update_count = self.data.update_count;
        Ok(BookResponse {
            correlation_id: self.correlation_id,
            client_id: self.client_id,
            instrument_id: self.instrument_id,
            data: book,
            start: self.start,
            end: self.end,
            ts_init: self.ts_init,
            params: self.params.into_owned(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct BookState {
    instrument_id: InstrumentId,
    book_type: BookType,
    sequence: u64,
    ts_last: UnixNanos,
    update_count: u64,
    orders: Vec<BookOrder>,
    #[serde(default)]
    bid_batch_state: u8,
    #[serde(default)]
    ask_batch_state: u8,
}
