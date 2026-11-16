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
    instruments::InstrumentAny,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};

pub mod core;

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
        let mass_status = self.client.generate_mass_status(lookback_mins).await?;

        if let Some(mass_status) = &mass_status {
            anyhow::ensure!(
                mass_status.client_id == self.client_id,
                "Execution mass status client ID {} did not match source client {}",
                mass_status.client_id,
                self.client_id,
            );
            anyhow::ensure!(
                mass_status.account_id == self.account_id,
                "Execution mass status account ID {} did not match source account {}",
                mass_status.account_id,
                self.account_id,
            );
            anyhow::ensure!(
                mass_status.venue == self.venue,
                "Execution mass status venue {} did not match source venue {}",
                mass_status.venue,
                self.venue,
            );
        }

        Ok(mass_status)
    }

    /// Forwards an instrument update to the underlying execution client.
    pub fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.client.on_instrument(instrument);
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use async_trait::async_trait;
    use futures::future::FutureExt;
    use nautilus_common::clients::ExecutionClient;
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::{
        accounts::AccountAny,
        enums::OmsType,
        identifiers::{AccountId, ClientId, Venue},
        reports::ExecutionMassStatus,
        types::{AccountBalance, MarginBalance},
    };
    use rstest::rstest;

    use super::ExecutionClientAdapter;
    use crate::engine::stubs::StubExecutionClient;

    struct MassStatusClient {
        stub: StubExecutionClient,
        mass_status: ExecutionMassStatus,
        lookbacks: Rc<RefCell<Vec<Option<u64>>>>,
    }

    impl MassStatusClient {
        fn new(mass_status: ExecutionMassStatus, lookbacks: Rc<RefCell<Vec<Option<u64>>>>) -> Self {
            let stub = StubExecutionClient::new(
                ClientId::from("SIM"),
                AccountId::from("SIM-001"),
                Venue::from("SIM"),
                OmsType::Netting,
                None,
            );

            Self {
                stub,
                mass_status,
                lookbacks,
            }
        }
    }

    #[async_trait(?Send)]
    impl ExecutionClient for MassStatusClient {
        fn is_connected(&self) -> bool {
            self.stub.is_connected()
        }

        fn client_id(&self) -> ClientId {
            self.stub.client_id()
        }

        fn account_id(&self) -> AccountId {
            self.stub.account_id()
        }

        fn venue(&self) -> Venue {
            self.stub.venue()
        }

        fn oms_type(&self) -> OmsType {
            self.stub.oms_type()
        }

        fn get_account(&self) -> Option<AccountAny> {
            self.stub.get_account()
        }

        fn generate_account_state(
            &self,
            balances: Vec<AccountBalance>,
            margins: Vec<MarginBalance>,
            reported: bool,
            ts_event: UnixNanos,
            info: Option<Params>,
        ) -> anyhow::Result<()> {
            self.stub
                .generate_account_state(balances, margins, reported, ts_event, info)
        }

        fn start(&mut self) -> anyhow::Result<()> {
            self.stub.start()
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            self.stub.stop()
        }

        async fn generate_mass_status(
            &self,
            lookback_mins: Option<u64>,
        ) -> anyhow::Result<Option<ExecutionMassStatus>> {
            self.lookbacks.borrow_mut().push(lookback_mins);

            Ok(Some(self.mass_status.clone()))
        }
    }

    #[rstest]
    #[case::matching_identity("SIM", "SIM-001", "SIM", None)]
    #[case::mismatched_client_id(
        "OTHER",
        "SIM-001",
        "SIM",
        Some("Execution mass status client ID OTHER did not match source client SIM")
    )]
    #[case::mismatched_account_id(
        "SIM",
        "OTHER-001",
        "SIM",
        Some("Execution mass status account ID OTHER-001 did not match source account SIM-001")
    )]
    #[case::mismatched_venue(
        "SIM",
        "SIM-001",
        "OTHER",
        Some("Execution mass status venue OTHER did not match source venue SIM")
    )]
    fn test_generate_mass_status_validates_identity_and_forwards_lookback(
        #[case] report_client_id: &str,
        #[case] report_account_id: &str,
        #[case] report_venue: &str,
        #[case] expected_error: Option<&str>,
    ) {
        let mass_status = ExecutionMassStatus::new(
            ClientId::from(report_client_id),
            AccountId::from(report_account_id),
            Venue::from(report_venue),
            UnixNanos::from(42),
            None,
        );

        let lookbacks = Rc::new(RefCell::new(Vec::new()));
        let client = MassStatusClient::new(mass_status.clone(), Rc::clone(&lookbacks));
        let adapter = ExecutionClientAdapter::new(Box::new(client));

        let result = adapter
            .generate_mass_status(Some(23))
            .now_or_never()
            .expect("mass status stub should complete immediately");

        if let Some(expected_error) = expected_error {
            assert_eq!(result.unwrap_err().to_string(), expected_error);
        } else {
            assert_eq!(result.unwrap(), Some(mass_status));
        }

        assert_eq!(lookbacks.borrow().as_slice(), &[Some(23)]);
    }
}
