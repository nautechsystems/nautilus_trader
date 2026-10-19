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

//! Client trait definitions for data and execution clients.
//!
//! Provides the core trait interfaces that define how clients interact with
//! data providers and execution venues.

mod data;
mod execution;

use std::{
    any::type_name,
    fmt::{Debug, Display},
};

pub use data::DataClient;
pub use execution::{
    DEFAULT_POSITION_RECONCILIATION_TOLERANCE, ExecutionClient, ExecutionReportTask,
    generate_mass_status,
};
use nautilus_model::identifiers::ClientId;

#[inline(always)]
fn log_not_implemented<T>(client_id: ClientId, _cmd: &T) {
    log::warn!(
        "Handler not implemented: client {client_id}, command {}",
        type_name::<T>()
    );
}

/// Logs trusted command context without formatting the provider error.
///
/// Pass only public command metadata; omit credentials and free-form parameters.
/// Retains the error parameter for caller compatibility; its text is not logged.
#[inline(always)]
pub fn log_command_error<C: Debug, E: Display>(cmd: &C, _e: &E) {
    log::error!("Client command failed: {cmd:?}");
}
