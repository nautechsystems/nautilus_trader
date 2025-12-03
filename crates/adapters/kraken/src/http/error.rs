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

//! Error types for Kraken HTTP client operations.

use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub enum KrakenHttpError {
    NetworkError(String),
    ApiError(Vec<String>),
    ParseError(String),
    AuthenticationError(String),
    MissingCredentials,
}

impl Display for KrakenHttpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::ApiError(errors) => write!(f, "API error: {}", errors.join(", ")),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::AuthenticationError(msg) => write!(f, "Authentication error: {msg}"),
            Self::MissingCredentials => write!(f, "Missing credentials"),
        }
    }
}

impl std::error::Error for KrakenHttpError {}

impl From<anyhow::Error> for KrakenHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}
