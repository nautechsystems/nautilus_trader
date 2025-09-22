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

//! Execution client implementations for trading venue connectivity.

use std::fmt::Debug;

use async_trait::async_trait;
use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
    GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount, QueryOrder,
    SubmitOrder, SubmitOrderList,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};

pub mod base;

pub trait ExecutionClient {
    fn is_connected(&self) -> bool;
    fn client_id(&self) -> ClientId;
    fn account_id(&self) -> AccountId;
    fn venue(&self) -> Venue;
    fn oms_type(&self) -> OmsType;
    fn get_account(&self) -> Option<AccountAny>;

    /// Generates and publishes the account state event.
    ///
    /// # Errors
    ///
    /// Returns an error if generating the account state fails.
    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()>;

    /// Starts the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to start.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to stop.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Submits a single order command to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Submits a list of orders to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Modifies an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails.
    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels a specific order.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels all orders.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels a batch of orders.
    ///
    /// # Errors
    ///
    /// Returns an error if batch cancellation fails.
    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Queries the status of an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Queries the status of an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }
}

#[async_trait(?Send)]
pub trait LiveExecutionClient: ExecutionClient {
    /// Establishes a connection for live execution.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Disconnects the live execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    async fn disconnect(&mut self) -> anyhow::Result<()>;

    /// Generates a single order status report.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        log_not_implemented(cmd);
        Ok(None)
    }

    /// Generates multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if report generation fails.
    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        log_not_implemented(cmd);
        Ok(Vec::new())
    }

    /// Generates fill reports based on execution results.
    ///
    /// # Errors
    ///
    /// Returns an error if fill report generation fails.
    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        log_not_implemented(&cmd);
        Ok(Vec::new())
    }

    /// Generates position status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if generation fails.
    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        log_not_implemented(cmd);
        Ok(Vec::new())
    }

    /// Generates mass status for executions.
    ///
    /// # Errors
    ///
    /// Returns an error if status generation fails.
    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log_not_implemented(&lookback_mins);
        Ok(None)
    }
}

#[inline(always)]
fn log_not_implemented<T: Debug>(cmd: &T) {
    log::warn!("{cmd:?} – handler not implemented");
}
