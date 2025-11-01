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

//! Common types and models for the dYdX adapter.

use serde::{Deserialize, Serialize};

/// dYdX account information.
///
/// Represents a Cosmos SDK account with its address, account number,
/// and current sequence (nonce) for transaction ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAccount {
    /// Cosmos SDK address (dydx...).
    pub address: String,
    /// Account number from the blockchain.
    pub account_number: u64,
    /// Current sequence number (nonce) for transactions.
    pub sequence: u64,
}
