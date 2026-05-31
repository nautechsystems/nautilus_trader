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

//! WebSocket error taxonomy.

use nautilus_network::error::SendError;
use thiserror::Error;

/// Errors emitted by the Lighter WebSocket client.
#[derive(Debug, Error)]
pub enum LighterWsError {
    /// Underlying transport failure.
    #[error("network error: {0}")]
    Network(String),
    /// Send-side transport failure. Carries the structured [`SendError`]
    /// so retry classifiers can match on the variant rather than the formatted message.
    #[error("transport error: {0}")]
    Transport(#[from] SendError),
    /// Authentication failure.
    #[error("authentication error: {0}")]
    Authentication(String),
    /// Failed to parse a wire frame.
    #[error("parse error: {0}")]
    Parse(String),
    /// Generic client error.
    #[error("client error: {0}")]
    Client(String),
}
