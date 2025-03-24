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
pub mod cancel_all;
pub mod cancel_batch;
pub mod modify;
pub mod query;
pub mod reports;
pub mod submit;
pub mod submit_list;

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{ClientId, InstrumentId};
use strum::Display;

// Re-exports
pub use self::{
    cancel::CancelOrder, cancel_all::CancelAllOrders, cancel_batch::BatchCancelOrders,
    modify::ModifyOrder, query::QueryOrder, submit::SubmitOrder, submit_list::SubmitOrderList,
};

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
        }
    }

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
        }
    }
}
