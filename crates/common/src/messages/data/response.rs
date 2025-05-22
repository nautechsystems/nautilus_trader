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

use std::{any::Any, sync::Arc};

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, DataType, QuoteTick, TradeTick},
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl CustomDataResponse {
    /// Creates a new [`CustomDataResponse`] instance.
    pub fn new<T: Any + Send + Sync>(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Option<Venue>,
        data_type: DataType,
        data: T,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data_type,
            data: Arc::new(data),
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl InstrumentResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`InstrumentResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: InstrumentAny,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl InstrumentsResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`InstrumentsResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data: Vec<InstrumentAny>,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data,
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl BookResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BookResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: OrderBook,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl QuotesResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`QuotesResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Vec<QuoteTick>,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
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
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl TradesResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`TradesResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        instrument_id: InstrumentId,
        data: Vec<TradeTick>,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            instrument_id,
            data,
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
    pub params: Option<IndexMap<String, String>>,
}

impl BarsResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BarsResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        bar_type: BarType,
        data: Vec<Bar>,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            bar_type,
            data,
            ts_init,
            params,
        }
    }
}
