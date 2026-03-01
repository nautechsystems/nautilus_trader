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

//! WebSocket error types for the Kalshi adapter.

use thiserror::Error;

/// Errors that can occur in the Kalshi WebSocket client.
#[derive(Debug, Error)]
pub enum KalshiWsError {
    /// WebSocket connection failed.
    #[error("WebSocket connection error: {0}")]
    Connection(String),
    /// Sequence number gap detected — re-subscribe required.
    #[error("Sequence gap on sid={sid}: expected {expected}, got {got}")]
    SequenceGap { sid: u32, expected: u64, got: u64 },
    /// JSON parsing failed.
    #[error("WebSocket JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    /// Authentication required for WebSocket connection.
    #[error("Authentication required for WebSocket connection")]
    NoCredential,
}
