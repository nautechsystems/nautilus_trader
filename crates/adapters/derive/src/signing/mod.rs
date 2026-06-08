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

//! Derive self-custodial action signing.
//!
//! Derive signs every state-changing request with an EIP-712 typed-data
//! signature over secp256k1, against per-action module contracts on the Derive
//! Chain. The pipeline (matching `derive_action_signing/signed_action.py` from
//! the upstream Python SDK [`derivexyz/v2-action-signing-python`]) is:
//!
//! ```text
//! action_hash = keccak256(abi.encode(
//!     [bytes32, uint, uint, address, bytes32, uint, address, address],
//!     [ACTION_TYPEHASH, subaccount_id, nonce, module_address,
//!      keccak256(module_data_abi_encoded), signature_expiry_sec, owner, signer],
//! ))
//! typed_data_hash = keccak256(0x1901 || DOMAIN_SEPARATOR || action_hash)
//! signature = secp256k1_sign(typed_data_hash, signer_key)
//! ```
//!
//! Per-action `module_data` ABI encodings live under [`modules`]. REST and
//! WebSocket session authentication use the simpler `eth_sign(timestamp_ms)`
//! pattern in [`auth`]. Per-`(wallet, subaccount)` nonce allocation lives in
//! [`nonce`].
//!
//! Differential testing against the upstream Python SDK is the byte-equivalence
//! oracle: fixtures are captured from `derive_action_signing` and replayed
//! against this implementation under `test_data/`.
//!
//! Protocol constants (`DOMAIN_SEPARATOR`, `ACTION_TYPEHASH`, module addresses)
//! are sourced from the Protocol Constants reference at
//! <https://docs.derive.xyz>.
//!
//! [`derivexyz/v2-action-signing-python`]: https://github.com/derivexyz/v2-action-signing-python

pub mod auth;
pub mod context;
pub mod eip712;
pub mod encoding;
pub mod modules;
pub mod nonce;
