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

//! Execution specific messages such as order commands.

pub mod cancel;
pub mod modify;
pub mod query;
pub mod report;
pub mod submit;

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use strum::Display;

// Re-exports
pub use self::{
    cancel::BatchCancelOrders, cancel::CancelAllOrders, cancel::CancelOrder, modify::ModifyOrder,
    query::QueryAccount, query::QueryOrder, report::GenerateFillReports,
    report::GenerateOrderStatusReport, report::GeneratePositionReports, submit::SubmitOrder,
    submit::SubmitOrderList,
};

/// Execution report variants for reconciliation.
#[derive(Clone, Debug, Display)]
pub enum ExecutionReport {
    OrderStatus(Box<OrderStatusReport>),
    Fill(Box<FillReport>),
    Position(Box<PositionStatusReport>),
    Mass(Box<ExecutionMassStatus>),
}

// TODO
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq, Display)]
pub enum TradingCommand {
    SubmitOrder(SubmitOrder),
    SubmitOrderList(SubmitOrderList),
    ModifyOrder(ModifyOrder),
    CancelOrder(CancelOrder),
    CancelAllOrders(CancelAllOrders),
    BatchCancelOrders(BatchCancelOrders),
    QueryOrder(QueryOrder),
    QueryAccount(QueryAccount),
}

impl TradingCommand {
    #[must_use]
    pub const fn client_id(&self) -> ClientId {
        match self {
            Self::SubmitOrder(command) => command.client_id,
            Self::SubmitOrderList(command) => command.client_id,
            Self::ModifyOrder(command) => command.client_id,
            Self::CancelOrder(command) => command.client_id,
            Self::CancelAllOrders(command) => command.client_id,
            Self::BatchCancelOrders(command) => command.client_id,
            Self::QueryOrder(command) => command.client_id,
            Self::QueryAccount(command) => command.client_id,
        }
    }

    /// Returns the instrument ID for the command.
    ///
    /// # Panics
    ///
    /// Panics if the command is `QueryAccount` which does not have an instrument ID.
    #[must_use]
    pub const fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::SubmitOrder(command) => command.instrument_id,
            Self::SubmitOrderList(command) => command.instrument_id,
            Self::ModifyOrder(command) => command.instrument_id,
            Self::CancelOrder(command) => command.instrument_id,
            Self::CancelAllOrders(command) => command.instrument_id,
            Self::BatchCancelOrders(command) => command.instrument_id,
            Self::QueryOrder(command) => command.instrument_id,
            Self::QueryAccount(_) => panic!("No instrument ID for command"),
        }
    }

    #[must_use]
    pub const fn ts_init(&self) -> UnixNanos {
        match self {
            Self::SubmitOrder(command) => command.ts_init,
            Self::SubmitOrderList(command) => command.ts_init,
            Self::ModifyOrder(command) => command.ts_init,
            Self::CancelOrder(command) => command.ts_init,
            Self::CancelAllOrders(command) => command.ts_init,
            Self::BatchCancelOrders(command) => command.ts_init,
            Self::QueryOrder(command) => command.ts_init,
            Self::QueryAccount(command) => command.ts_init,
        }
    }
}
