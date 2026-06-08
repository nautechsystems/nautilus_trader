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

//! Per-action `module_data` ABI encoders.
//!
//! Each Derive self-custodial action targets a dedicated module contract on
//! the Derive Chain. The ABI-encoded module data is keccak-hashed and folded
//! into the EIP-712 action hash assembled in [`super::eip712`].
//!
//! Initial scope: trade-module signing only. Withdraw / transfer / deposit /
//! RFQ encoders land here as scope expands.

pub mod trade;

/// Boxed error returned by [`ModuleData::to_abi_encoded`].
///
/// Each per-module encoder defines its own typed error (e.g.
/// [`trade::TradeEncodeError`]) and erases it through this alias so the
/// trait stays type-erased without forcing every caller to enumerate
/// every concrete module variant.
pub type ModuleEncodeError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Data encodable into a module-specific ABI payload that participates in
/// the EIP-712 action hash.
pub trait ModuleData {
    /// ABI-encode this module payload using the field tuple defined in the
    /// upstream Solidity action contract.
    ///
    /// # Errors
    ///
    /// Returns a [`ModuleEncodeError`] when the payload contains a value the
    /// venue contract cannot accept (e.g. a negative `max_fee` for trades or
    /// a decimal that overflows the 1e18-scaled signed/unsigned 256-bit
    /// range).
    fn to_abi_encoded(&self) -> Result<Vec<u8>, ModuleEncodeError>;
}
