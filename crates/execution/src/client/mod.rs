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

use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    types::{AccountBalance, MarginBalance},
};

use crate::{
    messages::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryOrder, SubmitOrder,
        SubmitOrderList,
        reports::{GenerateFillReports, GenerateOrderStatusReport, GeneratePositionReports},
    },
    reports::{
        fill::FillReport, mass_status::ExecutionMassStatus, order::OrderStatusReport,
        position::PositionStatusReport,
    },
};

pub mod base;

pub trait ExecutionClient {
    fn is_connected(&self) -> bool;
    fn client_id(&self) -> ClientId;
    fn account_id(&self) -> AccountId;
    fn venue(&self) -> Venue;
    fn oms_type(&self) -> OmsType;
    fn get_account(&self) -> Option<AccountAny>;
    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()>;
    fn start(&mut self) -> anyhow::Result<()>;
    fn stop(&mut self) -> anyhow::Result<()>;
    fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()>;
    fn submit_order_list(&self, command: SubmitOrderList) -> anyhow::Result<()>;
    fn modify_order(&self, command: ModifyOrder) -> anyhow::Result<()>;
    fn cancel_order(&self, command: CancelOrder) -> anyhow::Result<()>;
    fn cancel_all_orders(&self, command: CancelAllOrders) -> anyhow::Result<()>;
    fn batch_cancel_orders(&self, command: BatchCancelOrders) -> anyhow::Result<()>;
    fn query_order(&self, command: QueryOrder) -> anyhow::Result<()>;
}

pub trait LiveExecutionClient: ExecutionClient {
    fn connect(&self) -> anyhow::Result<()>;
    fn disconnect(&self) -> anyhow::Result<()>;
    fn generate_order_status_report(
        &self,
        report: GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>>;
    fn generate_order_status_reports(
        &self,
        report: GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>>;
    fn generate_fill_reports(&self, report: GenerateFillReports)
    -> anyhow::Result<Vec<FillReport>>;
    fn generate_position_status_reports(
        &self,
        report: GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>>;
    fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>>;
}
