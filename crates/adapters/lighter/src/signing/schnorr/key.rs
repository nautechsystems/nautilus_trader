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

//! Schnorr keypair types.
//!
//! [`PrivateKey`] is a thin wrapper over [`Scalar`] that derives its public
//! counterpart by computing `pk = (sk * G).encode()` through the
//! constant-time scalar multiplication path. [`PublicKey`] holds the canonical
//! `Fp5` encoding `w` and decodes back to a curve point at verification time.
//!
//! Both types expose the 40-byte canonical little-endian wire format Lighter
//! uses on the L2. Decoding accepts non-canonical scalar bytes and reduces them
//! modulo the group order, mirroring `ScalarElementFromLittleEndianBytes` from
//! the Go reference.

use std::fmt::Debug;

use super::sig::Signature;
use crate::signing::{
    curve::{Point, SCALAR_BYTES, Scalar},
    field::Fp5,
};

/// Canonical 40-byte little-endian length of a [`PublicKey`] (`Fp5` encoding).
const PUBLIC_KEY_BYTES: usize = 40;

/// A Schnorr private key over the ECgFp5 scalar field.
///
/// The wrapped [`Scalar`] is canonical (`< n`). Intentionally non-`Copy` so the
/// type cannot be silently duplicated past a future `Drop`/zeroize owner; the
/// `Debug` impl is redacted so accidental logging cannot leak the secret limbs.
/// Memory zeroization of secret material is deferred to the live signing
/// wire-up (Phase G), where the long-lived key store will own the
/// `PrivateKey` and apply `zeroize` on drop.
#[derive(Clone)]
pub struct PrivateKey(Scalar);

impl Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateKey(<redacted>)")
    }
}

impl PrivateKey {
    /// Wrap a canonical scalar as a private key. The caller is responsible for
    /// ensuring `s` is canonical and uniformly random in `1..n`.
    #[inline]
    #[must_use]
    pub fn from_scalar(s: Scalar) -> Self {
        Self(s)
    }

    /// Decode a private key from 40 little-endian bytes, reducing modulo the
    /// group order if necessary (matching the Go reference's scalar decoder).
    #[inline]
    #[must_use]
    pub fn from_le_bytes_reduce(bytes: [u8; SCALAR_BYTES]) -> Self {
        Self(Scalar::from_le_bytes_reduce(bytes))
    }

    /// Borrow the underlying canonical scalar.
    #[inline]
    #[must_use]
    pub fn as_scalar(&self) -> Scalar {
        self.0
    }

    /// Canonical 40-byte little-endian encoding of the private scalar.
    #[inline]
    #[must_use]
    pub fn to_le_bytes(&self) -> [u8; SCALAR_BYTES] {
        self.0.to_le_bytes()
    }

    /// Derive the matching public key as `pk = (sk * G).encode()`.
    ///
    /// Routes through the constant-time scalar mul so the secret scalar's
    /// limbs do not leak via timing.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(Point::mulgen_ct(self.0).encode())
    }

    /// Sign a pre-hashed message under the supplied per-signature nonce `k`.
    ///
    /// `hashed_msg` is the `Fp5` digest produced by the caller (typically via
    /// [`crate::signing::hash::hash_to_quintic_extension`] over the message
    /// field elements, or via [`crate::signing::tx::sign_tx`] which folds
    /// the body and attribute hashes). `k` MUST be drawn uniformly at random
    /// from a cryptographic RNG, MUST NOT be zero (a zero nonce trivially
    /// reveals `sk` from the resulting signature), and MUST NOT repeat across
    /// distinct signatures under the same key (a repeated nonce reveals `sk`
    /// from any two signatures sharing it). Matching the Go reference
    /// `SchnorrSignHashedMessage2`, the caller-contract is enforced by the
    /// caller — no runtime `k != 0` check is performed inside `sign`.
    #[inline]
    #[must_use]
    pub fn sign(&self, hashed_msg: Fp5, k: Scalar) -> Signature {
        super::sig::sign(self.0, hashed_msg, k)
    }
}

/// A Schnorr public key over the ECgFp5 curve, stored as the canonical
/// `Fp5` encoding `w = (sk * G).encode()`.
///
/// The wire format used by Lighter's L2 protocol is the 40-byte little-endian
/// representation of this `Fp5` element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PublicKey(Fp5);

impl PublicKey {
    /// Wrap an existing `Fp5` encoding as a public key. No curve check is
    /// performed here; [`Self::verify`] will reject the key if it does not
    /// decode to a valid group element.
    #[inline]
    #[must_use]
    pub fn from_fp5(w: Fp5) -> Self {
        Self(w)
    }

    /// Borrow the underlying `Fp5` encoding.
    #[inline]
    #[must_use]
    pub fn as_fp5(&self) -> Fp5 {
        self.0
    }

    /// Decode 40 little-endian bytes into a public key. Returns `None` if any
    /// 8-byte limb is non-canonical (`>= p`).
    ///
    /// Matches the Go reference's `FromCanonicalLittleEndianBytes`, which
    /// rejects any limb whose `u64` value is `>= p`. Phase E Layer 2 oracle
    /// tests confirm the closed mainnet signer always emits canonical bytes
    /// out of `ToLittleEndianBytesF`, so the strict policy round-trips
    /// without exception. No reducing variant is needed; non-canonical input
    /// would only ever come from a malformed or adversarial peer.
    #[inline]
    #[must_use]
    pub fn try_from_le_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Option<Self> {
        Fp5::try_from_le_bytes(bytes).map(Self)
    }

    /// Canonical 40-byte little-endian encoding of the public key.
    #[inline]
    #[must_use]
    pub fn to_le_bytes(&self) -> [u8; PUBLIC_KEY_BYTES] {
        self.0.to_le_bytes()
    }

    /// Verify a signature against this public key for the given pre-hashed
    /// message. Returns `false` for any decode failure or if the recovered
    /// challenge differs from the signature's `e` component.
    #[inline]
    #[must_use]
    pub fn verify(&self, hashed_msg: Fp5, sig: &Signature) -> bool {
        super::sig::verify(self.0, hashed_msg, sig)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::signing::field::MODULUS;

    #[rstest]
    fn private_key_debug_redacts_secret_limbs() {
        let secret_pattern = [0xABu8; SCALAR_BYTES];
        let sk = PrivateKey::from_le_bytes_reduce(secret_pattern);
        let formatted = format!("{sk:?}");

        assert_eq!(formatted, "PrivateKey(<redacted>)");
        assert!(
            !formatted.contains("ab") && !formatted.contains("AB"),
            "Debug must not leak secret bytes, was {formatted}",
        );
    }

    #[rstest]
    fn try_from_le_bytes_accepts_canonical_pubkey() {
        let pk_bytes = PrivateKey::from_le_bytes_reduce([0x42; SCALAR_BYTES])
            .public_key()
            .to_le_bytes();
        let parsed = PublicKey::try_from_le_bytes(pk_bytes)
            .expect("canonical pk bytes must round trip through try_from_le_bytes");
        assert_eq!(parsed.to_le_bytes(), pk_bytes);
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(2)]
    #[case(3)]
    #[case(4)]
    fn try_from_le_bytes_rejects_non_canonical_limb(#[case] limb_index: usize) {
        let mut bytes = [0u8; PUBLIC_KEY_BYTES];
        bytes[limb_index * 8..(limb_index + 1) * 8].copy_from_slice(&MODULUS.to_le_bytes());
        assert!(
            PublicKey::try_from_le_bytes(bytes).is_none(),
            "limb {limb_index} == MODULUS must be rejected",
        );

        bytes[limb_index * 8..(limb_index + 1) * 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(
            PublicKey::try_from_le_bytes(bytes).is_none(),
            "limb {limb_index} == u64::MAX must be rejected",
        );
    }
}
