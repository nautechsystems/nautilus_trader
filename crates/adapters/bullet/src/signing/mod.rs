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

//! Transaction signing for the Bullet exchange.
//!
//! Bullet uses Borsh-serialized signed transactions with an Ed25519 signature over
//! `borsh(UnsignedTransaction) ++ chain_hash` (32-byte domain separator).
//!
//! See: <https://tradingapi.bullet.xyz/docs/tx-signing.md>

pub mod chain_data;
pub mod tx_builder;
pub mod uniqueness;
