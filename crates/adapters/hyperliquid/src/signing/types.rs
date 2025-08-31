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

use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Unique identifier for a signer (API wallet or user address).
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerId(pub String);

impl Display for SignerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SignerId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SignerId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Hyperliquid action types for different signing schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HyperliquidActionType {
    /// L1 actions (agent deposits, withdrawals) - signed with L1 scheme.
    L1,
    /// User actions (trading) - signed with user-signed scheme.
    UserSigned,
}
