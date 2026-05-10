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

//! Chain parameters fetched from `GET /fapi/v1/exchangeInfo`.
//!
//! `chain_id` and `chain_hash` are environment-specific constants that change only on schema
//! rotation. We fetch them once at startup and cache them; if the exchange returns 401
//! "Invalid signature", the caller should refresh and re-sign.

use crate::common::{error::BulletError, models::ExchangeInfo};

/// Chain parameters required to sign and validate transactions.
#[derive(Debug, Clone)]
pub struct ChainData {
    /// Chain identifier embedded in every `TxDetails`.
    pub chain_id: u64,
    /// 32-byte domain separator appended to the borsh-serialized `UnsignedTransaction`
    /// before signing.
    pub chain_hash: [u8; 32],
}

impl ChainData {
    /// Build `ChainData` from an `ExchangeInfo` response.
    ///
    /// # Errors
    ///
    /// Returns an error if `chainHash` is absent or malformed, or if `chainInfo` is absent.
    pub fn from_exchange_info(info: &ExchangeInfo) -> Result<Self, BulletError> {
        let chain_hash = info.decode_chain_hash()?;
        let chain_id = info
            .chain_info
            .as_ref()
            .map(|ci| ci.chain_id)
            .ok_or_else(|| BulletError::Parse("exchangeInfo missing chainInfo".to_string()))?;
        Ok(Self { chain_id, chain_hash })
    }
}
