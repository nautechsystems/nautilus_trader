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

//! Shared fixture decoders for the signing module's vector tests.
//!
//! Each fixture-driven test module decoded vectors with its own ad-hoc copy of
//! these routines. This module centralises the lowercase-hex codec and a
//! handful of typed decoders so test files can focus on the assertion content.
//! The module is gated by `#[cfg(test)]` and is not part of the public API.

use std::fmt::Write;

use proptest::prelude::*;

use super::{
    curve::{Point, SCALAR_BYTES, Scalar},
    field::{Fp, Fp5},
    schnorr::SIG_BYTES,
};

/// Decode a lowercase hex string into a byte vector.
///
/// Panics on odd length or non-hex characters; both are test-only contracts.
#[must_use]
pub(crate) fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "hex must have even length");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Lowercase hex encoding of a byte slice (no `0x` prefix).
#[must_use]
pub(crate) fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").expect("writing into String never fails");
    }
    s
}

/// Decode a fixed-length hex string into a `[u8; N]` array.
///
/// Panics if `hex.len() != N * 2`.
#[must_use]
pub(crate) fn hex_to_array<const N: usize>(hex: &str) -> [u8; N] {
    let bytes = hex_to_bytes(hex);
    assert_eq!(bytes.len(), N, "expected {N} bytes, was {}", bytes.len(),);
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    out
}

/// Decode 80-hex-char input into a 40-byte LE scalar buffer.
#[must_use]
pub(crate) fn decode_scalar_bytes(hex: &str) -> [u8; SCALAR_BYTES] {
    hex_to_array::<SCALAR_BYTES>(hex)
}

/// Decode 80-hex-char input into the 40-byte canonical `Fp5` encoding.
///
/// Panics if the bytes do not decode as a canonical `Fp5` (any limb `>= p`).
#[must_use]
pub(crate) fn decode_fp5_bytes(hex: &str) -> Fp5 {
    Fp5::try_from_le_bytes(hex_to_array::<40>(hex)).expect("non-canonical Fp5 fixture")
}

/// Decode 160-hex-char input into a 80-byte LE Schnorr signature buffer.
#[must_use]
pub(crate) fn decode_sig_bytes(hex: &str) -> [u8; SIG_BYTES] {
    hex_to_array::<SIG_BYTES>(hex)
}

/// Strategy for an arbitrary [`Fp`]: feeds a raw `u64` through the reducing
/// constructor so the result lands in canonical range.
pub(crate) fn arb_fp() -> impl Strategy<Value = Fp> {
    any::<u64>().prop_map(Fp::from_u64_reduce)
}

/// Strategy for an arbitrary non-zero [`Fp`]. Uses `prop_filter` so the
/// shrinker does not collapse to zero.
pub(crate) fn arb_fp_nonzero() -> impl Strategy<Value = Fp> {
    arb_fp().prop_filter("non-zero", |x| !x.is_zero())
}

/// Strategy for an arbitrary [`Fp5`]: each coefficient is reduced into `Fp`
/// so the result is always canonical.
pub(crate) fn arb_fp5() -> impl Strategy<Value = Fp5> {
    any::<[u64; 5]>().prop_map(Fp5::from_u64s_reduce)
}

/// Strategy for an arbitrary non-zero [`Fp5`].
pub(crate) fn arb_fp5_nonzero() -> impl Strategy<Value = Fp5> {
    arb_fp5().prop_filter("non-zero", |x| !x.is_zero())
}

/// Strategy for an arbitrary canonical [`Scalar`]: feeds five raw `u64`
/// limbs through the reducing decoder so the result always lands in `0..n`.
pub(crate) fn arb_scalar() -> impl Strategy<Value = Scalar> {
    any::<[u64; 5]>().prop_map(|limbs| {
        let mut bytes = [0u8; SCALAR_BYTES];
        for (i, limb) in limbs.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
        }
        Scalar::from_le_bytes_reduce(bytes)
    })
}

/// Strategy for an arbitrary non-zero canonical [`Scalar`].
pub(crate) fn arb_scalar_nonzero() -> impl Strategy<Value = Scalar> {
    arb_scalar().prop_filter("non-zero", |s| !s.is_zero())
}

/// Strategy for an arbitrary curve point in the prime-order subgroup,
/// constructed as `seed * G` for an arbitrary seed scalar. The neutral and
/// the generator are part of the seed range.
pub(crate) fn arb_point() -> impl Strategy<Value = Point> {
    arb_scalar().prop_map(|seed| Point::GENERATOR.scalar_mul(seed))
}
