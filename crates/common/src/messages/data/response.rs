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

use std::{any::Any, ops::Range, sync::Arc};

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, BatchView, DataType, FundingRateUpdate, HasTsInit, OrderBookDelta,
        OrderBookDepth, QuoteTick, TradeTick,
    },
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    types::Price,
};
use serde::{Deserialize, Serialize};

use super::{
    IntoInstrumentIds, Payload,
    identifiers::{deserialize_instrument_ids, single_instrument_id},
};

// Bounds are inclusive on ts_init; trimming leaves interior rows unchanged
pub(crate) fn trim_data_to_bounds<T: HasTsInit>(
    data: &mut Vec<T>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) {
    let range = range_for_bounds(data, start, end);
    data.drain(..range.start);
    data.truncate(range.len());
}

pub(crate) fn trim_batch_to_bounds<T: HasTsInit>(
    data: &mut BatchView<T>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) {
    let range = range_for_bounds(data, start, end);
    if range != (0..data.len()) {
        *data = data.slice(range.start, range.end);
    }
}

fn range_for_bounds<T: HasTsInit>(
    data: &[T],
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> Range<usize> {
    let first = start.map_or(0, |start| {
        data.iter()
            .position(|item| item.ts_init() >= start)
            .unwrap_or(data.len())
    });

    let end = end.map_or(data.len(), |end| {
        data.iter()
            .rposition(|item| item.ts_init() <= end)
            .map_or(0, |index| index + 1)
    });

    first..end.max(first)
}

#[derive(Clone, Debug)]
pub struct CustomDataResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub data: Payload,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl CustomDataResponse {
    /// Creates a new [`CustomDataResponse`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new<T: Any + Send + Sync>(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Option<Venue>,
        data_type: DataType,
        data: T,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data_type,
            data: Arc::new(data),
            start,
            end,
            ts_init,
            params,
        }
    }

    /// Converts the response to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstrumentResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: InstrumentAny,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl InstrumentResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`InstrumentResponse`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: InstrumentAny,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstrumentsResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data: Vec<InstrumentAny>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl InstrumentsResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`InstrumentsResponse`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data: Vec<InstrumentAny>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data,
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BookResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: OrderBook,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl BookResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BookResponse`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: OrderBook,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookDeltasResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: BatchView<OrderBookDelta>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl BookDeltasResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BookDeltasResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: impl Into<BatchView<OrderBookDelta>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data: data.into(),
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookDepthResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: BatchView<OrderBookDepth>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl BookDepthResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BookDepthResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: impl Into<BatchView<OrderBookDepth>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data: data.into(),
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub data: BatchView<QuoteTick>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl QuotesResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`QuotesResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_ids: impl IntoInstrumentIds,
        data: impl Into<BatchView<QuoteTick>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            correlation_id,
            client_id,
            instrument_ids,
            data: data.into(),
            start,
            end,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member response path.
    ///
    /// # Errors
    ///
    /// Returns an error if the response does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "response")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub data: BatchView<TradeTick>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl TradesResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`TradesResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_ids: impl IntoInstrumentIds,
        data: impl Into<BatchView<TradeTick>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            correlation_id,
            client_id,
            instrument_ids,
            data: data.into(),
            start,
            end,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member response path.
    ///
    /// # Errors
    ///
    /// Returns an error if the response does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "response")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingRatesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: BatchView<FundingRateUpdate>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl FundingRatesResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`FundingRatesResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: impl Into<BatchView<FundingRateUpdate>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data: data.into(),
            start,
            end,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionChainReferencePriceResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub series_id: OptionSeriesId,
    pub price: Option<Price>,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl OptionChainReferencePriceResponse {
    /// Creates a new [`OptionChainReferencePriceResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        series_id: OptionSeriesId,
        price: Option<Price>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            series_id,
            price,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BarsResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub bar_type: BarType,
    pub data: BatchView<Bar>,
    pub ts_init: UnixNanos,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub params: Option<Params>,
}

impl BarsResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BarsResponse`] instance.
    ///
    /// Accepts an owned vector, a shared buffer, or a batch view.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        bar_type: BarType,
        data: impl Into<BatchView<Bar>>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            bar_type,
            data: data.into(),
            ts_init,
            start,
            end,
            params,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::data::stubs::quote_audusd;
    use rstest::{fixture, rstest};

    use super::*;
    use crate::messages::data::DataResponse;

    #[fixture]
    fn quote_rows() -> Vec<QuoteTick> {
        let mut first = quote_audusd();
        first.ts_event = UnixNanos::from(7);
        first.ts_init = UnixNanos::from(11);
        let mut second = first;
        second.ts_event = UnixNanos::from(17);
        second.ts_init = UnixNanos::from(23);
        vec![first, second]
    }

    #[rstest]
    fn test_quote_response_shares_supplied_buffer(quote_rows: Vec<QuoteTick>) {
        let instrument_id = quote_rows[0].instrument_id;
        let rows = Arc::new(quote_rows);
        let request_id = UUID4::new();
        let client_id = ClientId::from("HISTORY");
        let start = Some(UnixNanos::from(5));
        let end = Some(UnixNanos::from(29));
        let ts_init = UnixNanos::from(31);
        let params = Some(Params::new([("source".into(), "history".into())].into()));

        let response = QuotesResponse::new(
            request_id,
            client_id,
            instrument_id,
            Arc::clone(&rows),
            start,
            end,
            ts_init,
            params.clone(),
        );
        let serialized = serde_json::to_value(&response).unwrap();

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, client_id);
        assert_eq!(response.instrument_ids, vec![instrument_id]);
        assert_eq!(response.data.as_ref(), rows.as_slice());
        assert!(Arc::ptr_eq(response.data.arc(), &rows));
        assert_eq!(response.start, start);
        assert_eq!(response.end, end);
        assert_eq!(response.ts_init, ts_init);
        assert_eq!(response.params, params);
        assert_eq!(
            serialized["data"],
            serde_json::to_value(rows.as_ref()).unwrap()
        );
    }

    #[rstest]
    fn test_quote_response_vector_constructor_keeps_row_allocation(quote_rows: Vec<QuoteTick>) {
        let instrument_id = quote_rows[0].instrument_id;
        let rows = quote_rows.clone();
        let row_ptr = rows.as_ptr();

        let response = QuotesResponse::new(
            UUID4::new(),
            ClientId::from("HISTORY"),
            instrument_id,
            rows,
            None,
            None,
            UnixNanos::from(31),
            None,
        );

        assert_eq!(response.data.as_ref(), &quote_rows);
        assert_eq!(response.data.as_ptr(), row_ptr);
    }

    #[rstest]
    #[case::unbounded(None, None)]
    #[case::exact(Some(11), Some(23))]
    #[case::wider(Some(5), Some(29))]
    fn test_shared_response_trim_keeps_existing_allocation(
        quote_rows: Vec<QuoteTick>,
        #[case] start: Option<u64>,
        #[case] end: Option<u64>,
    ) {
        let instrument_id = quote_rows[0].instrument_id;
        let rows = Arc::new(quote_rows);
        let mut response = DataResponse::Quotes(QuotesResponse::new(
            UUID4::new(),
            ClientId::from("HISTORY"),
            instrument_id,
            Arc::clone(&rows),
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
            UnixNanos::from(31),
            None,
        ));
        response.trim_to_bounds();

        let DataResponse::Quotes(response) = response else {
            unreachable!()
        };

        assert_eq!(response.data.as_ref(), rows.as_slice());
        assert!(Arc::ptr_eq(response.data.arc(), &rows));
    }

    #[rstest]
    fn test_shared_response_trim_preserves_other_consumers(quote_rows: Vec<QuoteTick>) {
        let instrument_id = quote_rows[0].instrument_id;
        let rows = Arc::new(quote_rows.clone());
        let mut response = DataResponse::Quotes(QuotesResponse::new(
            UUID4::new(),
            ClientId::from("HISTORY"),
            instrument_id,
            Arc::clone(&rows),
            Some(UnixNanos::from(23)),
            Some(UnixNanos::from(23)),
            UnixNanos::from(31),
            None,
        ));
        response.trim_to_bounds();

        let DataResponse::Quotes(response) = response else {
            unreachable!()
        };

        assert_eq!(rows.as_ref(), &quote_rows);
        assert_eq!(response.data.as_ref(), &vec![quote_rows[1]]);
        assert!(Arc::ptr_eq(response.data.arc(), &rows));
        assert_eq!(response.data.range(), 1..2);
    }

    #[rstest]
    #[case::unbounded(vec![11, 23], None, None, 0..2)]
    #[case::inclusive(vec![11, 23], Some(11), Some(23), 0..2)]
    #[case::first(vec![11, 23], None, Some(11), 0..1)]
    #[case::last(vec![11, 23], Some(23), None, 1..2)]
    #[case::after(vec![11, 23], Some(29), None, 2..2)]
    #[case::before(vec![11, 23], None, Some(5), 0..0)]
    #[case::reversed(vec![11, 23], Some(23), Some(11), 1..1)]
    #[case::unsorted(vec![11, 37, 23], Some(11), Some(23), 0..3)]
    #[case::empty(Vec::new(), Some(11), Some(23), 0..0)]
    fn test_response_trim_preserves_narrowed_backing(
        #[case] timestamps: Vec<u64>,
        #[case] start: Option<u64>,
        #[case] end: Option<u64>,
        #[case] range: Range<usize>,
    ) {
        let visible: Vec<_> = timestamps
            .into_iter()
            .map(|ts| QuoteTick {
                ts_init: UnixNanos::from(ts),
                ..Default::default()
            })
            .collect();

        let mut rows = vec![QuoteTick::default()];
        rows.extend_from_slice(&visible);
        rows.push(QuoteTick {
            ts_init: UnixNanos::from(9999),
            ..Default::default()
        });

        let rows = Arc::new(rows);
        let mut view = BatchView::new(Arc::clone(&rows), 1..visible.len() + 1);

        trim_batch_to_bounds(
            &mut view,
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
        );

        assert_eq!(view.as_ref(), &visible[range.clone()]);
        assert_eq!(view.range(), (1 + range.start)..(1 + range.end));
        assert!(Arc::ptr_eq(view.arc(), &rows));
        assert_eq!(&rows[1..rows.len() - 1], visible.as_slice());
    }
}
