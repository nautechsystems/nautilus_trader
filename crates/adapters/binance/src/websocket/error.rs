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

//! Binance WebSocket error types.

use std::fmt;

/// Binance WebSocket client error type.
#[derive(Debug)]
pub enum BinanceWsError {
    /// General client error.
    ClientError(String),
    /// Authentication failed.
    AuthenticationError(String),
    /// Message parsing error.
    ParseError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Operation timed out.
    Timeout(String),
}

impl fmt::Display for BinanceWsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientError(msg) => write!(f, "Client error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
        }
    }
}

impl std::error::Error for BinanceWsError {}

/// Result type for Binance WebSocket operations.
pub type BinanceWsResult<T> = Result<T, BinanceWsError>;
