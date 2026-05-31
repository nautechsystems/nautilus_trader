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

//! Schnorr signatures over the ECgFp5 curve, with Poseidon2 as the binding hash.
//!
//! - [`PrivateKey`] wraps a curve scalar; [`PublicKey`] wraps the canonical
//!   `Fp5` encoding `w = (sk * G).encode()`.
//! - [`Signature`] is the `(s, e)` pair encoded as `s_le || e_le` over 80 bytes.
//! - [`PrivateKey::sign`] produces a signature for a pre-hashed message under a
//!   caller-supplied per-signature nonce `k`. Production callers must draw `k`
//!   from a cryptographic RNG; the explicit-nonce form mirrors the Go
//!   reference's `SchnorrSignHashedMessage2`, which fixture tests pin against.
//! - [`PublicKey::verify`] decodes the public-key encoding, recomputes
//!   `s*G + e*pk`, and checks the recovered challenge against the signature.
//!
//! Hash-to-quintic of the message is the caller's responsibility, matching the
//! upstream API. Use [`super::hash::hash_to_quintic_extension`] over the
//! message field elements.

mod key;
mod sig;

pub use key::{PrivateKey, PublicKey};
pub use sig::{SIG_BYTES, Signature};
