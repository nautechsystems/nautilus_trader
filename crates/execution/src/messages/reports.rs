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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ClientOrderId, InstrumentId};

pub struct GenerateOrderStatusReport {
    command_id: UUID4,
    ts_init: UnixNanos,
    instrument_id: Option<InstrumentId>,
    client_order_id: Option<ClientOrderId>,
    venue_order_id: Option<ClientOrderId>,
}

impl GenerateOrderStatusReport {
    #[must_use]
    pub const fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<ClientOrderId>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            client_order_id,
            venue_order_id,
        }
    }
}

pub struct GenerateOrderStatusReports {
    command_id: UUID4,
    ts_init: UnixNanos,
    open_only: bool,
    instrument_id: Option<InstrumentId>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
}

impl GenerateOrderStatusReports {
    #[must_use]
    pub const fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        open_only: bool,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            open_only,
            instrument_id,
            start,
            end,
        }
    }
}

pub struct GenerateFillReports {
    command_id: UUID4,
    ts_init: UnixNanos,
    instrument_id: Option<InstrumentId>,
    venue_order_id: Option<ClientOrderId>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
}

impl GenerateFillReports {
    #[must_use]
    pub const fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<ClientOrderId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            venue_order_id,
            start,
            end,
        }
    }
}

pub struct GeneratePositionReports {
    command_id: UUID4,
    ts_init: UnixNanos,
    instrument_id: Option<InstrumentId>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
}

impl GeneratePositionReports {
    #[must_use]
    pub const fn new(
        command_id: UUID4,
        ts_init: UnixNanos,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> Self {
        Self {
            command_id,
            ts_init,
            instrument_id,
            start,
            end,
        }
    }
}
