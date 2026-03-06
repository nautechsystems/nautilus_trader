use std::{any::Any, sync::Arc};

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, DataType, FundingRateUpdate, QuoteTick, TradeTick},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
};

use super::Payload;

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
    #[allow(clippy::too_many_arguments)]
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

#[derive(Clone, Debug)]
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
    #[allow(clippy::too_many_arguments)]
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

#[derive(Clone, Debug)]
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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

#[derive(Clone, Debug)]
pub struct QuotesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: Vec<QuoteTick>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Vec<QuoteTick>,
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

#[derive(Clone, Debug)]
pub struct TradesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: Vec<TradeTick>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Vec<TradeTick>,
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

#[derive(Clone, Debug)]
pub struct FundingRatesResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub instrument_id: InstrumentId,
    pub data: Vec<FundingRateUpdate>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Vec<FundingRateUpdate>,
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

#[derive(Clone, Debug)]
pub struct BarsResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub bar_type: BarType,
    pub data: Vec<Bar>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        bar_type: BarType,
        data: Vec<Bar>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            bar_type,
            data,
            ts_init,
            start,
            end,
            params,
        }
    }
}
