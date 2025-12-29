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

//! Binance adapter error types.

use std::fmt;

use crate::{http::error::BinanceHttpError, websocket::error::BinanceWsError};

/// Top-level Binance adapter error type.
#[derive(Debug)]
pub enum BinanceError {
    /// HTTP client error.
    Http(BinanceHttpError),
    /// WebSocket client error.
    WebSocket(BinanceWsError),
    /// Configuration error.
    Config(String),
    /// Data parsing error.
    Parse(String),
}

impl fmt::Display for BinanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::WebSocket(e) => write!(f, "WebSocket error: {e}"),
            Self::Config(msg) => write!(f, "Configuration error: {msg}"),
            Self::Parse(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

impl std::error::Error for BinanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::WebSocket(e) => Some(e),
            Self::Config(_) | Self::Parse(_) => None,
        }
    }
}

impl From<BinanceHttpError> for BinanceError {
    fn from(err: BinanceHttpError) -> Self {
        Self::Http(err)
    }
}

impl From<BinanceWsError> for BinanceError {
    fn from(err: BinanceWsError) -> Self {
        Self::WebSocket(err)
    }
}

/// Result type for Binance adapter operations.
pub type BinanceResult<T> = Result<T, BinanceError>;
