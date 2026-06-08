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

//! Schnorr `(s, e)` signature container, signing routine, and verification.
//!
//! The signature scheme follows the elliottech `poseidon_crypto` reference:
//!
//! Sign:
//!     `r = (k * G).encode()` (`Fp5`)
//!     `e = HashToQuintic(r || hashed_msg)` reduced into the scalar field
//!     `s = k - e * sk`
//!
//! Verify:
//!     decode `pk` to a curve point; reject malformed encodings
//!     `r_v = (s * G + e * pk).encode()`
//!     `e_v = HashToQuintic(r_v || hashed_msg)` reduced into the scalar field
//!     accept iff `e_v == e`
//!
//! Signing routes through the constant-time scalar mul; verification uses the
//! variable-time path because all of its inputs are public. Signature bytes are
//! laid out as `s_le || e_le` over 80 bytes, matching the Go reference's
//! `Signature.ToBytes` / `SigFromBytes`.

use crate::signing::{
    curve::{Point, SCALAR_BYTES, Scalar},
    field::Fp5,
    hash::hash_two_to_quintic,
};

/// Canonical wire length of a [`Signature`]: 40 bytes for `s`, 40 for `e`.
pub const SIG_BYTES: usize = 2 * SCALAR_BYTES;

/// Schnorr signature over the ECgFp5 curve.
///
/// `s` and `e` are canonical scalars (`< n`). Wire format is `s_le || e_le`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Signature {
    /// `s = k - e * sk` component.
    pub s: Scalar,
    /// `e = H(r || H(m))` component reduced into the scalar field.
    pub e: Scalar,
}

impl Signature {
    /// Encode to the canonical 80-byte little-endian layout `s_le || e_le`.
    #[must_use]
    pub fn to_le_bytes(&self) -> [u8; SIG_BYTES] {
        let mut out = [0u8; SIG_BYTES];
        out[..SCALAR_BYTES].copy_from_slice(&self.s.to_le_bytes());
        out[SCALAR_BYTES..].copy_from_slice(&self.e.to_le_bytes());
        out
    }

    /// Decode an 80-byte signature, reducing each scalar limb sequence modulo
    /// the group order if it exceeds the canonical range.
    ///
    /// Mirrors the Go reference's `SigFromBytes` semantics: non-canonical
    /// encodings are accepted and reduced. The verification routine performs
    /// the actual canonicality / membership check.
    #[must_use]
    pub fn from_le_bytes_reduce(bytes: [u8; SIG_BYTES]) -> Self {
        let mut s_buf = [0u8; SCALAR_BYTES];
        let mut e_buf = [0u8; SCALAR_BYTES];
        s_buf.copy_from_slice(&bytes[..SCALAR_BYTES]);
        e_buf.copy_from_slice(&bytes[SCALAR_BYTES..]);

        Self {
            s: Scalar::from_le_bytes_reduce(s_buf),
            e: Scalar::from_le_bytes_reduce(e_buf),
        }
    }

    /// Test whether both scalar components are canonical (`< n`).
    #[must_use]
    pub fn is_canonical(&self) -> bool {
        self.s.is_canonical() && self.e.is_canonical()
    }
}

/// Sign a pre-hashed message under the supplied per-signature nonce `k`.
///
/// See [`super::PrivateKey::sign`] for the public entry point.
#[must_use]
pub(super) fn sign(sk: Scalar, hashed_msg: Fp5, k: Scalar) -> Signature {
    let r = Point::mulgen_ct(k).encode();
    let e = Scalar::from_fp5(hash_two_to_quintic(r, hashed_msg));

    Signature { s: k - e * sk, e }
}

/// Verify a signature against the public-key encoding `pk_w` for the given
/// pre-hashed message. See [`super::PublicKey::verify`] for the public entry.
///
/// No explicit "neutral public key" rejection is applied. ECgFp5 is a
/// prime-order group, so `Point::decode` either returns a member of the prime
/// subgroup or rejects the encoding outright; the recurring "subgroup check"
/// concern from cofactor curves does not apply. The Go reference
/// `IsSchnorrSignatureValid` follows the same contract, and Phase E Layer 2
/// oracle tests confirm the mainnet signer accepts the same set of encodings.
#[must_use]
pub(super) fn verify(pk_w: Fp5, hashed_msg: Fp5, sig: &Signature) -> bool {
    if !sig.is_canonical() {
        return false;
    }

    let pk = match Point::decode(pk_w) {
        Some(p) => p,
        None => return false,
    };

    let r_v = (Point::mulgen(sig.s) + pk.scalar_mul(sig.e)).encode();
    let e_v = Scalar::from_fp5(hash_two_to_quintic(r_v, hashed_msg));

    e_v == sig.e
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;
    use crate::signing::{
        fixtures::{
            arb_fp5, arb_scalar, bytes_to_hex, decode_fp5_bytes, decode_scalar_bytes,
            decode_sig_bytes,
        },
        schnorr::{PrivateKey, PublicKey},
    };

    const VECTORS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_schnorr_vectors.json",
    ));

    #[derive(Debug, Deserialize)]
    struct VectorsFile {
        vectors: Vectors,
    }

    #[derive(Debug, Deserialize)]
    struct Vectors {
        keygen: Vec<KeyGenVector>,
        sign: Vec<SignVector>,
    }

    #[derive(Debug, Deserialize)]
    struct KeyGenVector {
        sk: String,
        pk: String,
    }

    #[derive(Debug, Deserialize)]
    struct SignVector {
        sk: String,
        hashed_msg: String,
        k: String,
        sig: String,
    }

    #[rstest]
    fn keygen_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.keygen.is_empty(), "keygen vectors empty");

        for (i, v) in suite.vectors.keygen.iter().enumerate() {
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let pk = sk.public_key();
            assert_eq!(
                bytes_to_hex(&pk.to_le_bytes()),
                v.pk,
                "vector {i}: pk encoding diverged",
            );
        }
    }

    #[rstest]
    fn sign_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.sign.is_empty(), "sign vectors empty");

        for (i, v) in suite.vectors.sign.iter().enumerate() {
            let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
            let hashed_msg = decode_fp5_bytes(&v.hashed_msg);
            let k = Scalar::from_le_bytes_reduce(decode_scalar_bytes(&v.k));

            let sig = sk.sign(hashed_msg, k);
            assert_eq!(
                bytes_to_hex(&sig.to_le_bytes()),
                v.sig,
                "vector {i}: signature bytes diverged",
            );

            let pk = sk.public_key();
            assert!(pk.verify(hashed_msg, &sig), "vector {i}: verify failed");
        }
    }

    #[rstest]
    fn signature_round_trip_through_bytes() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        let v = &suite.vectors.sign[0];

        let bytes = decode_sig_bytes(&v.sig);
        let sig = Signature::from_le_bytes_reduce(bytes);
        assert!(sig.is_canonical(), "fixture sig must be canonical");
        assert_eq!(sig.to_le_bytes(), bytes, "round trip diverged");
    }

    #[rstest]
    fn verify_rejects_corrupted_signature() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        let v = &suite.vectors.sign[0];

        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        let hashed_msg = decode_fp5_bytes(&v.hashed_msg);
        let k = Scalar::from_le_bytes_reduce(decode_scalar_bytes(&v.k));
        let mut sig = sk.sign(hashed_msg, k);
        let pk = sk.public_key();

        sig.s += Scalar::ONE;
        assert!(!pk.verify(hashed_msg, &sig), "tampered sig must not verify");
    }

    #[rstest]
    fn verify_rejects_wrong_message() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        let v = &suite.vectors.sign[0];

        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        let hashed_msg = decode_fp5_bytes(&v.hashed_msg);
        let k = Scalar::from_le_bytes_reduce(decode_scalar_bytes(&v.k));
        let sig = sk.sign(hashed_msg, k);
        let pk = sk.public_key();

        let other_msg = Fp5::from_u64s_reduce([1, 2, 3, 4, 5]);
        assert!(
            !pk.verify(other_msg, &sig),
            "verify must fail for a different message",
        );
    }

    #[rstest]
    fn verify_rejects_malformed_pubkey() {
        // Use one of the bad encodings checked by the curve decode tests.
        let bad = Fp5::from_u64s_reduce([
            13_557_832_913_345_268_708,
            15_669_280_705_791_538_619,
            8_534_654_657_267_986_396,
            12_533_218_303_838_131_749,
            5_058_070_698_878_426_028,
        ]);
        let pk = crate::signing::schnorr::PublicKey::from_fp5(bad);

        let sig = Signature {
            s: Scalar::ONE,
            e: Scalar::ONE,
        };
        assert!(
            !pk.verify(Fp5::ZERO, &sig),
            "verify must fail when pk does not decode",
        );
    }

    /// Returns a non-canonical `Scalar` whose limb sequence equals the group
    /// order: `ORDER` itself fails `is_canonical` (the predicate is strict
    /// less-than) and is the cheapest non-canonical witness available without
    /// reaching for `add_inner`.
    fn non_canonical_scalar() -> Scalar {
        crate::signing::curve::ORDER
    }

    #[rstest]
    #[case(true, true, true)]
    #[case(false, true, false)]
    #[case(true, false, false)]
    #[case(false, false, false)]
    fn signature_is_canonical_truth_table(
        #[case] s_canonical: bool,
        #[case] e_canonical: bool,
        #[case] expected: bool,
    ) {
        let canon = Scalar::ONE;
        let non_canon = non_canonical_scalar();
        let sig = Signature {
            s: if s_canonical { canon } else { non_canon },
            e: if e_canonical { canon } else { non_canon },
        };
        assert_eq!(sig.is_canonical(), expected);
    }

    #[rstest]
    fn verify_rejects_non_canonical_signature() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        let v = &suite.vectors.sign[0];

        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        let hashed_msg = decode_fp5_bytes(&v.hashed_msg);
        let pk = sk.public_key();

        let bad = non_canonical_scalar();
        let sig = Signature { s: bad, e: bad };

        assert!(
            !pk.verify(hashed_msg, &sig),
            "verify must reject non-canonical signature scalars",
        );
    }

    proptest! {
        /// Round-trip property: any canonical `(sk, k != 0, hashed_msg)` yields
        /// a signature that verifies under the matching public key.
        #[rstest]
        fn prop_sign_verify_round_trip(
            sk in arb_scalar(),
            k in arb_scalar(),
            hashed_msg in arb_fp5(),
        ) {
            // Schnorr is undefined for k == 0 (caller contract); skip that
            // case rather than asserting on a malformed input.
            prop_assume!(!k.is_zero());
            // sk == 0 is also pathological (pk = neutral); skip to avoid
            // tripping the deferred neutral-pk concern unrelated to the
            // round-trip property.
            prop_assume!(!sk.is_zero());

            let private_key = PrivateKey::from_scalar(sk);
            let pk = private_key.public_key();
            let sig = private_key.sign(hashed_msg, k);
            prop_assert!(pk.verify(hashed_msg, &sig));
        }

        /// Round-trip through bytes: any canonical signature decodes back to
        /// the same value via `from_le_bytes_reduce`.
        #[rstest]
        fn prop_signature_bytes_round_trip(
            sk in arb_scalar(),
            k in arb_scalar(),
            hashed_msg in arb_fp5(),
        ) {
            prop_assume!(!sk.is_zero());
            prop_assume!(!k.is_zero());
            let sig = PrivateKey::from_scalar(sk).sign(hashed_msg, k);
            prop_assert!(sig.is_canonical());
            let decoded = Signature::from_le_bytes_reduce(sig.to_le_bytes());
            prop_assert_eq!(decoded, sig);
        }
    }

    proptest! {
        // Bit-flip rejection: perturbing any byte of `s`, `e`, or `pk` by a
        // single-bit flip MUST cause verification to fail. Each case runs a
        // sign + verify, so we keep the case count moderate.
        #![proptest_config(ProptestConfig {
            cases: 64,
            ..ProptestConfig::default()
        })]

        /// Flipping any bit in `sig.s` causes verify to fail.
        #[rstest]
        fn prop_verify_rejects_single_bit_flip_in_s(
            sk in arb_scalar(),
            k in arb_scalar(),
            hashed_msg in arb_fp5(),
            byte_idx in 0usize..SCALAR_BYTES,
            bit in 0u8..8,
        ) {
            prop_assume!(!sk.is_zero());
            prop_assume!(!k.is_zero());
            let private_key = PrivateKey::from_scalar(sk);
            let pk = private_key.public_key();
            let sig = private_key.sign(hashed_msg, k);

            let mut bytes = sig.to_le_bytes();
            bytes[byte_idx] ^= 1 << bit;
            let tampered = Signature::from_le_bytes_reduce(bytes);
            prop_assume!(tampered != sig);
            prop_assert!(!pk.verify(hashed_msg, &tampered));
        }

        /// Flipping any bit in `sig.e` causes verify to fail.
        #[rstest]
        fn prop_verify_rejects_single_bit_flip_in_e(
            sk in arb_scalar(),
            k in arb_scalar(),
            hashed_msg in arb_fp5(),
            byte_idx in SCALAR_BYTES..SIG_BYTES,
            bit in 0u8..8,
        ) {
            prop_assume!(!sk.is_zero());
            prop_assume!(!k.is_zero());
            let private_key = PrivateKey::from_scalar(sk);
            let pk = private_key.public_key();
            let sig = private_key.sign(hashed_msg, k);

            let mut bytes = sig.to_le_bytes();
            bytes[byte_idx] ^= 1 << bit;
            let tampered = Signature::from_le_bytes_reduce(bytes);
            prop_assume!(tampered != sig);
            prop_assert!(!pk.verify(hashed_msg, &tampered));
        }

        /// Perturbing the public key by a non-zero offset causes verify to
        /// fail (overwhelmingly: either the perturbed `Fp5` does not decode,
        /// or it decodes to a different group point and the recovered `e_v`
        /// differs from `sig.e`).
        #[rstest]
        fn prop_verify_rejects_perturbed_pubkey(
            sk in arb_scalar(),
            k in arb_scalar(),
            hashed_msg in arb_fp5(),
            offset in arb_fp5(),
        ) {
            prop_assume!(!sk.is_zero());
            prop_assume!(!k.is_zero());
            prop_assume!(!offset.is_zero());

            let private_key = PrivateKey::from_scalar(sk);
            let pk = private_key.public_key();
            let sig = private_key.sign(hashed_msg, k);
            prop_assert!(pk.verify(hashed_msg, &sig));

            let perturbed = PublicKey::from_fp5(pk.as_fp5() + offset);
            // The original pk encoding plus offset is almost surely a
            // different element; if by accident it equals pk, skip.
            prop_assume!(perturbed.as_fp5() != pk.as_fp5());
            prop_assert!(!perturbed.verify(hashed_msg, &sig));
        }
    }

    /// Pin the documented "no neutral public-key rejection" contract:
    /// `Fp5::ZERO` decodes as the curve neutral, and verifying a random
    /// signature under it returns `false` cleanly (no panic).
    #[rstest]
    fn verify_under_neutral_pubkey_returns_false() {
        let pk = PublicKey::from_fp5(Fp5::ZERO);
        let sig = Signature {
            s: Scalar::ONE,
            e: Scalar::ONE,
        };
        assert!(
            !pk.verify(Fp5::from_u64s_reduce([1, 2, 3, 4, 5]), &sig),
            "verify under neutral pk must return false for an unrelated sig",
        );
    }

    #[rstest]
    fn from_le_bytes_reduce_normalizes_non_canonical_signature() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        let v = &suite.vectors.sign[0];

        let sk = PrivateKey::from_le_bytes_reduce(decode_scalar_bytes(&v.sk));
        let hashed_msg = decode_fp5_bytes(&v.hashed_msg);
        let k = Scalar::from_le_bytes_reduce(decode_scalar_bytes(&v.k));
        let canonical = sk.sign(hashed_msg, k);
        let pk = sk.public_key();

        // Mirrors the Go `TestBytes` case: add ORDER to each scalar without
        // reducing, encode raw, and feed back through the reducing decoder.
        let s_inflated = canonical.s.add_inner(crate::signing::curve::ORDER);
        let e_inflated = canonical.e.add_inner(crate::signing::curve::ORDER);
        let mut bytes = [0u8; SIG_BYTES];
        bytes[..SCALAR_BYTES].copy_from_slice(&s_inflated.to_le_bytes());
        bytes[SCALAR_BYTES..].copy_from_slice(&e_inflated.to_le_bytes());

        let reduced = Signature::from_le_bytes_reduce(bytes);
        assert!(
            reduced.is_canonical(),
            "reduced signature must be canonical",
        );
        assert_eq!(
            reduced, canonical,
            "reduction must recover the canonical sig"
        );
        assert!(
            pk.verify(hashed_msg, &reduced),
            "reduced signature must still verify",
        );
    }
}
