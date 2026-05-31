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

//! Poseidon2 hash over the Goldilocks field used by the Lighter signer.
//!
//! Exposes the fixed-width permutation [`permute`] together with the sponge
//! API the Lighter Schnorr binding consumes ([`hash_no_pad`],
//! [`hash_two_to_one`], [`hash_n_to_one`], [`hash_to_quintic_extension`]).
//! Parameter constants live in [`params`] and were transcribed from the
//! Apache-2.0 Go reference; equivalence is verified by fixture vectors under
//! `test_data/`.

pub mod params;
mod poseidon2;

pub use params::{RATE, ROUNDS_F, ROUNDS_F_HALF, ROUNDS_P, WIDTH};
pub use poseidon2::{
    HASH_OUT, hash_n_to_hash_no_pad, hash_n_to_m_no_pad, hash_n_to_one, hash_no_pad,
    hash_to_quintic_extension, hash_two_to_one, hash_two_to_quintic, permute,
};
