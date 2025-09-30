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

//! Message types for system communication.
//!
//! This module provides message types used for communication between different
//! parts of the NautilusTrader system, including data requests, execution commands,
//! and system control messages.

use nautilus_model::{data::Data, events::OrderEventAny};
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
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Display)]
pub enum DataEvent {
    Response(DataResponse),
    Data(Data),
    #[cfg(feature = "defi")]
    DeFi(nautilus_model::defi::data::DefiData),
}

/// Execution event variants for order events and reports.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Display)]
pub enum ExecutionEvent {
    Order(OrderEventAny),
    Report(ExecutionReport),
}
