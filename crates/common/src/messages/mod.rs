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

//! Message types for system communication.
//!
//! This module provides message types used for communication between different
//! parts of the NautilusTrader system, including data requests, execution commands,
//! and system control messages.

use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus, option_chain::OptionGreeks},
    events::{
        AccountState, OrderAcceptedBatch, OrderCanceledBatch, OrderEventAny, OrderSubmittedBatch,
    },
    instruments::InstrumentAny,
};
use strum::Display;

pub mod data;
pub mod execution;
pub mod system;

#[cfg(feature = "defi")]
pub mod defi;

// Re-exports
pub use data::{DataResponse, SubscribeCommand, UnsubscribeCommand};
pub use execution::ExecutionReport;

// TODO: Refine this to reduce disparity between enum sizes
#[allow(
    clippy::large_enum_variant,
    reason = "event enum keeps all data variants in one routing type"
)]
#[derive(Debug, Display)]
pub enum DataEvent {
    Response(DataResponse),
    Data(Data),
    // Kept separate from `Data` pending the decision on generic dispatch versus this routing enum
    Instrument(InstrumentAny),
    FundingRate(FundingRateUpdate),
    InstrumentStatus(InstrumentStatus),
    OptionGreeks(OptionGreeks),
    // nautilus-import-ok: conditional compilation import
    #[cfg(feature = "defi")]
    DeFi(nautilus_model::defi::data::DefiData),
}

/// System command variants routed to a live node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum SystemCommand {
    #[strum(transparent)]
    ReconnectSocket(system::ReconnectSocket),
}

/// System event variants routed to a live node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum SystemEvent {
    #[strum(transparent)]
    SocketState(system::SocketStateChange),
}

/// Execution event variants for order events and reports.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Display)]
pub enum ExecutionEvent {
    #[strum(transparent)]
    Order(OrderEventAny),
    #[strum(transparent)]
    OrderSubmittedBatch(OrderSubmittedBatch),
    #[strum(transparent)]
    OrderAcceptedBatch(OrderAcceptedBatch),
    #[strum(transparent)]
    OrderCanceledBatch(OrderCanceledBatch),
    #[strum(transparent)]
    Report(ExecutionReport),
    #[strum(transparent)]
    Account(AccountState),
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::AccountType,
        events::OrderInitialized,
        identifiers::{AccountId, ClientId, TraderId, Venue},
        reports::ExecutionMassStatus,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::messages::system::{ReconnectSocket, SocketState, SocketStateChange};

    #[rstest]
    fn system_messages_delegate_display_to_inner() {
        let command = ReconnectSocket::new(
            TraderId::from("TRADER-001"),
            ClientId::from("BINANCE"),
            Ustr::from("orders"),
            UnixNanos::from(1),
        );
        let event = SocketStateChange::new(
            ClientId::from("BINANCE"),
            Some(Venue::from("BINANCE")),
            Ustr::from("orders"),
            SocketState::Connected,
        );
        let command_expected = command.to_string();
        let event_expected = event.to_string();

        assert_eq!(
            SystemCommand::ReconnectSocket(command).to_string(),
            command_expected
        );
        assert_eq!(SystemEvent::SocketState(event).to_string(), event_expected);
    }

    #[rstest]
    fn execution_events_delegate_display_to_inner() {
        let order = OrderEventAny::Initialized(OrderInitialized::default());
        let submitted_batch = OrderSubmittedBatch::new(Vec::new());
        let accepted_batch = OrderAcceptedBatch::new(Vec::new());
        let canceled_batch = OrderCanceledBatch::new(Vec::new());
        let report = ExecutionReport::MassStatus(Box::new(ExecutionMassStatus::new(
            ClientId::from("BINANCE"),
            AccountId::from("BINANCE-001"),
            Venue::from("BINANCE"),
            UnixNanos::from(2),
            Some(UUID4::from("00000000-0000-4000-8000-000000000001")),
        )));
        let account = AccountState::new(
            AccountId::from("BINANCE-001"),
            AccountType::Cash,
            Vec::new(),
            Vec::new(),
            true,
            UUID4::from("00000000-0000-4000-8000-000000000002"),
            UnixNanos::from(3),
            UnixNanos::from(4),
            None,
        );
        let cases = [
            (ExecutionEvent::Order(order.clone()), order.to_string()),
            (
                ExecutionEvent::OrderSubmittedBatch(submitted_batch.clone()),
                submitted_batch.to_string(),
            ),
            (
                ExecutionEvent::OrderAcceptedBatch(accepted_batch.clone()),
                accepted_batch.to_string(),
            ),
            (
                ExecutionEvent::OrderCanceledBatch(canceled_batch.clone()),
                canceled_batch.to_string(),
            ),
            (ExecutionEvent::Report(report.clone()), report.to_string()),
            (
                ExecutionEvent::Account(account.clone()),
                account.to_string(),
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(event.to_string(), expected);
        }
    }
}
