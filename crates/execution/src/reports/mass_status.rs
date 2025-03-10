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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId};
use serde::{Deserialize, Serialize};

use crate::reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport};

/// Represents an execution mass status report for an execution client - including
/// status of all orders, trades for those orders and open positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.execution")
)]
pub struct ExecutionMassStatus {
    /// The client ID for the report.
    pub client_id: ClientId,
    /// The account ID for the report.
    pub account_id: AccountId,
    /// The venue for the report.
    pub venue: Venue,
    /// The report ID.
    pub report_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: UnixNanos,
    /// The order status reports.
    order_reports: IndexMap<VenueOrderId, OrderStatusReport>,
    /// The fill reports.
    fill_reports: IndexMap<VenueOrderId, Vec<FillReport>>,
    /// The position status reports.
    position_reports: IndexMap<InstrumentId, Vec<PositionStatusReport>>,
}

impl ExecutionMassStatus {
    /// Creates a new execution mass status report.
    #[must_use]
    pub fn new(
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        ts_init: UnixNanos,
        report_id: Option<UUID4>,
    ) -> Self {
        Self {
            client_id,
            account_id,
            venue,
            report_id: report_id.unwrap_or_default(),
            ts_init,
            order_reports: IndexMap::new(),
            fill_reports: IndexMap::new(),
            position_reports: IndexMap::new(),
        }
    }

    /// Get a copy of the order reports map.
    #[must_use]
    pub fn order_reports(&self) -> IndexMap<VenueOrderId, OrderStatusReport> {
        self.order_reports.clone()
    }

    /// Get a copy of the fill reports map.
    #[must_use]
    pub fn fill_reports(&self) -> IndexMap<VenueOrderId, Vec<FillReport>> {
        self.fill_reports.clone()
    }

    /// Get a copy of the position reports map.
    #[must_use]
    pub fn position_reports(&self) -> IndexMap<InstrumentId, Vec<PositionStatusReport>> {
        self.position_reports.clone()
    }

    /// Add order reports to the mass status.
    pub fn add_order_reports(&mut self, reports: Vec<OrderStatusReport>) {
        for report in reports {
            self.order_reports.insert(report.venue_order_id, report);
        }
    }

    /// Add fill reports to the mass status.
    pub fn add_fill_reports(&mut self, reports: Vec<FillReport>) {
        for report in reports {
            self.fill_reports
                .entry(report.venue_order_id)
                .or_default()
                .push(report);
        }
    }

    /// Add position reports to the mass status.
    pub fn add_position_reports(&mut self, reports: Vec<PositionStatusReport>) {
        for report in reports {
            self.position_reports
                .entry(report.instrument_id)
                .or_default()
                .push(report);
        }
    }
}

impl std::fmt::Display for ExecutionMassStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ExecutionMassStatus(client_id={}, account_id={}, venue={}, order_reports={:?}, fill_reports={:?}, position_reports={:?}, report_id={}, ts_init={})",
            self.client_id,
            self.account_id,
            self.venue,
            self.order_reports,
            self.fill_reports,
            self.position_reports,
            self.report_id,
            self.ts_init,
        )
    }
}
