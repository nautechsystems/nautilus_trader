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

use std::fmt::Display;

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::identifiers::{ClientId, ClientOrderId, InstrumentId, TraderId, Venue};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateOrderStatusReport {
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub instrument_id: Option<InstrumentId>,
    pub client_order_id: Option<ClientOrderId>,
    pub venue_order_id: Option<ClientOrderId>,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl GenerateOrderStatusReport {
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<ClientOrderId>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            client_order_id,
            venue_order_id,
            params,
            correlation_id,
        }
    }
}

impl Display for GenerateOrderStatusReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, client_order_id={:?}, venue_order_id={:?}, command_id={})",
            stringify!(GenerateOrderStatusReport),
            self.instrument_id,
            self.client_order_id,
            self.venue_order_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateOrderStatusReports {
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub open_only: bool,
    pub instrument_id: Option<InstrumentId>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl GenerateOrderStatusReports {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        open_only: bool,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            open_only,
            instrument_id,
            start,
            end,
            params,
            correlation_id,
        }
    }
}

impl Display for GenerateOrderStatusReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(open_only={}, instrument_id={:?}, command_id={})",
            stringify!(GenerateOrderStatusReports),
            self.open_only,
            self.instrument_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateFillReports {
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub instrument_id: Option<InstrumentId>,
    pub venue_order_id: Option<ClientOrderId>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl GenerateFillReports {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<ClientOrderId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            venue_order_id,
            start,
            end,
            params,
            correlation_id,
        }
    }
}

impl Display for GenerateFillReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, venue_order_id={:?}, command_id={})",
            stringify!(GenerateFillReports),
            self.instrument_id,
            self.venue_order_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratePositionStatusReports {
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub instrument_id: Option<InstrumentId>,
    pub start: Option<UnixNanos>,
    pub end: Option<UnixNanos>,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl GeneratePositionStatusReports {
    #[must_use]
    pub fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            start,
            end,
            params,
            correlation_id,
        }
    }
}

impl Display for GeneratePositionStatusReports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={:?}, command_id={})",
            stringify!(GeneratePositionStatusReports),
            self.instrument_id,
            self.command_id,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateExecutionMassStatus {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl GenerateExecutionMassStatus {
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            command_id,
            ts_init,
            params,
            correlation_id,
        }
    }
}

impl Display for GenerateExecutionMassStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, client_id={}, venue={:?}, command_id={})",
            stringify!(GenerateExecutionMassStatus),
            self.trader_id,
            self.client_id,
            self.venue,
            self.command_id,
        )
    }
}
