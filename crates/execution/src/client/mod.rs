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

//! Execution client implementations for trading venue connectivity.

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use nautilus_common::messages::execution::{
    GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
    GeneratePositionStatusReports,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OmsType,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};

pub mod base;

use nautilus_common::clients::ExecutionClient;

/// Wraps an [`ExecutionClient`], managing its lifecycle and providing access to the client.
pub struct ExecutionClientAdapter {
    pub(crate) client: Box<dyn ExecutionClient>,
    pub client_id: ClientId,
    pub venue: Venue,
    pub account_id: AccountId,
    pub oms_type: OmsType,
}

impl Deref for ExecutionClientAdapter {
    type Target = Box<dyn ExecutionClient>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for ExecutionClientAdapter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Debug for ExecutionClientAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionClientAdapter))
            .field("client_id", &self.client_id)
            .field("venue", &self.venue)
            .field("account_id", &self.account_id)
            .field("oms_type", &self.oms_type)
            .finish()
    }
}

impl ExecutionClientAdapter {
    /// Creates a new [`ExecutionClientAdapter`] with the given client.
    #[must_use]
    pub fn new(client: Box<dyn ExecutionClient>) -> Self {
        let client_id = client.client_id();
        let venue = client.venue();
        let account_id = client.account_id();
        let oms_type = client.oms_type();

        Self {
            client,
            client_id,
            venue,
            account_id,
            oms_type,
        }
    }

    /// Connects the execution client to the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.connect().await
    }

    /// Disconnects the execution client from the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.disconnect().await
    }

    /// Generates a single order status report.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    pub async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.client.generate_order_status_report(cmd).await
    }

    /// Generates multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    pub async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.client.generate_order_status_reports(cmd).await
    }

    /// Generates fill reports based on execution results.
    ///
    /// # Errors
    ///
    /// Returns an error if fill report generation fails.
    pub async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.client.generate_fill_reports(cmd).await
    }

    /// Generates position status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    pub async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.client.generate_position_status_reports(cmd).await
    }

    /// Generates mass status for executions.
    ///
    /// # Errors
    ///
    /// Returns an error if status generation fails.
    pub async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        self.client.generate_mass_status(lookback_mins).await
    }

    /// Registers an external order for tracking by the execution client.
    ///
    /// This is called after reconciliation creates an external order, allowing the
    /// execution client to track it for subsequent events (e.g., cancellations).
    pub fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: UnixNanos,
    ) {
        self.client.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
            ts_init,
        );
    }
}
