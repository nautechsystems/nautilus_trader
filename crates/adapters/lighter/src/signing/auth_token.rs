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

//! Lighter REST/WebSocket auth-token builder.
//!
//! The Lighter venue authenticates long-running clients against a Schnorr
//! signature over an ASCII message describing `(deadline, account_index,
//! api_key_index)`. The signed string is sent verbatim in the `Authorization`
//! header and on the WebSocket subscribe handshake.
//!
//! Pipeline (matching the Go reference `ConstructAuthToken`):
//!
//! 1. Render `message = "{deadline}:{account_index}:{api_key_index}"`.
//! 2. Split the ASCII bytes into 8-byte little-endian chunks, zero-padding the
//!    final chunk if needed, and decode each chunk as a canonical Goldilocks
//!    `Fp` element.
//! 3. Run [`hash_to_quintic_extension`] over the resulting `[Fp]` to derive a
//!    single `Fp5` digest.
//! 4. Sign with the supplied `(sk, k)` using the standard Schnorr binding.
//! 5. Concatenate `"{message}:{hex(sig)}"` where `sig` is the canonical
//!    80-byte `s_le || e_le` Schnorr layout encoded as lowercase hex.
//!
//! The hash path here is not the body-element pipeline `tx::compute_tx_hash`
//! uses; the auth token treats the message as opaque ASCII and packs it into
//! limbs by raw little-endian bytes, while transactions encode each field as
//! an `Fp`-domain integer.

use std::{
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngExt;
use thiserror::Error;

use crate::{
    common::consts::LIGHTER_AUTH_TOKEN_MAX_TTL,
    signing::{
        curve::{SCALAR_BYTES, Scalar},
        field::Fp,
        hash::hash_to_quintic_extension,
        schnorr::{PrivateKey, SIG_BYTES},
    },
};

/// Errors raised by [`build_auth_token`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthTokenError {
    /// The supplied deadline is at or before `now`.
    #[error("auth-token deadline {deadline} is not in the future of now {now}")]
    DeadlineNotInFuture {
        /// Caller-supplied deadline (UNIX seconds).
        deadline: i64,
        /// Reference `now` (UNIX seconds).
        now: i64,
    },
    /// The supplied deadline exceeds the venue's maximum TTL.
    #[error("auth-token deadline {deadline} exceeds max TTL {max_ttl_secs}s from now {now}")]
    TtlTooLarge {
        /// Caller-supplied deadline (UNIX seconds).
        deadline: i64,
        /// Reference `now` (UNIX seconds).
        now: i64,
        /// Configured maximum TTL in seconds.
        max_ttl_secs: i64,
    },
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
    /// An 8-byte chunk of the message decoded to a non-canonical Goldilocks
    /// element (`>= p`).
    ///
    /// Cannot occur for the auth-token format (`"{deadline}:{account}:{key}"`)
    /// since every byte is ASCII; surfaced as a typed error rather than a
    /// panic so callers of [`hash_auth_message`] with arbitrary input get a
    /// recoverable failure.
    #[error("non-canonical Goldilocks limb at byte offset {offset}")]
    MessageEncoding {
        /// Byte offset of the offending 8-byte chunk in the input.
        offset: usize,
    },
}

/// Default deadline applied by [`build_auth_token_for`] when the caller does
/// not supply one: 7 hours from the current wall clock. Sits inside the
/// venue's [`LIGHTER_AUTH_TOKEN_MAX_TTL`] (8 hours) with an hour of head room
/// so a long-running session can pre-fetch and rotate before expiry.
pub const DEFAULT_AUTH_TOKEN_TTL_SECS: i64 = 7 * 60 * 60;

/// Mint an auth token from a [`crate::common::credential::Credential`] using
/// the default 7-hour TTL and a fresh CSPRNG nonce.
///
/// The token format matches the Go reference's `ConstructAuthToken`. The
/// returned string is the value the WebSocket subscribe handshake sends in
/// the `auth` field of an `account_*` channel subscription.
///
/// # Errors
///
/// Returns the underlying [`crate::common::credential::Credential::private_key`]
/// failure if the secret cannot be decoded, or any [`build_auth_token`]
/// failure (clock-before-epoch or, hypothetically, a deadline-validation
/// breach the helper itself sets).
pub fn build_auth_token_for(
    credential: &crate::common::credential::Credential,
) -> anyhow::Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("system clock is before UNIX epoch"))?
        .as_secs();
    let now_i64 = i64::try_from(now)
        .map_err(|_| anyhow::anyhow!("system clock overflowed when converting to i64"))?;
    let deadline = now_i64
        .checked_add(DEFAULT_AUTH_TOKEN_TTL_SECS)
        .ok_or_else(|| anyhow::anyhow!("deadline computation overflowed"))?;
    let sk = credential.private_key()?;
    build_auth_token(
        deadline,
        credential.account_index(),
        credential.api_key_index(),
        &sk,
        fresh_k(),
    )
    .map_err(|e| anyhow::anyhow!("failed to mint Lighter auth token: {e}"))
}

/// Draws a fresh canonical [`Scalar`] from the thread-local CSPRNG suitable
/// for the per-signature `k` nonce.
///
/// The Schnorr binding requires `k` to be drawn from a cryptographic RNG and
/// used at most once per signature; see [`PrivateKey::sign`] for the full
/// contract. The 40-byte draw is reduced modulo the curve order, so the
/// returned scalar is always canonical.
#[must_use]
pub fn fresh_k() -> Scalar {
    let mut bytes = [0u8; SCALAR_BYTES];
    rand::rng().fill(&mut bytes[..]);
    Scalar::from_le_bytes_reduce(bytes)
}

/// Build a Lighter auth token using the system clock as the `now` reference.
///
/// Validates the deadline against [`LIGHTER_AUTH_TOKEN_MAX_TTL`]. See
/// [`build_auth_token_at`] for an injectable-`now` variant suitable for tests.
///
/// # Errors
///
/// Returns [`AuthTokenError::DeadlineNotInFuture`] if `deadline_unix_secs <=
/// now`, [`AuthTokenError::TtlTooLarge`] if it exceeds the venue cap, or
/// [`AuthTokenError::ClockBeforeEpoch`] if the system clock predates the
/// UNIX epoch.
pub fn build_auth_token(
    deadline_unix_secs: i64,
    account_index: i64,
    api_key_index: u8,
    sk: &PrivateKey,
    k: Scalar,
) -> Result<String, AuthTokenError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthTokenError::ClockBeforeEpoch)?
        .as_secs();
    let now_i64 = i64::try_from(now).map_err(|_| AuthTokenError::ClockBeforeEpoch)?;
    build_auth_token_at(
        now_i64,
        deadline_unix_secs,
        account_index,
        api_key_index,
        sk,
        k,
    )
}

/// Variant of [`build_auth_token`] accepting an explicit `now_unix_secs`.
///
/// Pure of `SystemTime`, so callers can drive the validation deterministically
/// from a wall-clock provider or a test fixture. Same error semantics as
/// [`build_auth_token`].
///
/// # Errors
///
/// See [`build_auth_token`].
pub fn build_auth_token_at(
    now_unix_secs: i64,
    deadline_unix_secs: i64,
    account_index: i64,
    api_key_index: u8,
    sk: &PrivateKey,
    k: Scalar,
) -> Result<String, AuthTokenError> {
    if deadline_unix_secs <= now_unix_secs {
        return Err(AuthTokenError::DeadlineNotInFuture {
            deadline: deadline_unix_secs,
            now: now_unix_secs,
        });
    }

    let ttl_secs = deadline_unix_secs - now_unix_secs;
    let max_ttl_secs = i64::try_from(LIGHTER_AUTH_TOKEN_MAX_TTL.as_secs()).unwrap_or(i64::MAX);

    if ttl_secs > max_ttl_secs {
        return Err(AuthTokenError::TtlTooLarge {
            deadline: deadline_unix_secs,
            now: now_unix_secs,
            max_ttl_secs,
        });
    }

    build_auth_token_unchecked(deadline_unix_secs, account_index, api_key_index, sk, k)
}

/// Sign the auth-token message without TTL validation.
///
/// Public so tests and oracle round-trips can produce tokens whose deadline
/// would otherwise trip the venue cap. Production callers should use
/// [`build_auth_token`] or [`build_auth_token_at`].
///
/// # Errors
///
/// Returns [`AuthTokenError::MessageEncoding`] if the rendered message contains
/// an 8-byte chunk that does not decode as a canonical Goldilocks element. The
/// auth-token format keeps every byte ASCII, so this case is unreachable for
/// the production callers.
pub fn build_auth_token_unchecked(
    deadline_unix_secs: i64,
    account_index: i64,
    api_key_index: u8,
    sk: &PrivateKey,
    k: Scalar,
) -> Result<String, AuthTokenError> {
    let message = auth_token_message(deadline_unix_secs, account_index, api_key_index);
    let sig = sign_message(&message, sk, k)?;
    Ok(format_token(&message, &sig))
}

/// ASCII auth-token message: `"{deadline}:{account}:{api_key}"`.
///
/// Public for the rare caller that needs to recompute the signed preimage
/// (e.g., to verify a token against a known public key). The message format
/// matches the Go reference verbatim.
#[must_use]
pub fn auth_token_message(
    deadline_unix_secs: i64,
    account_index: i64,
    api_key_index: u8,
) -> String {
    format!("{deadline_unix_secs}:{account_index}:{api_key_index}")
}

/// Hash the auth-token ASCII message to its 40-byte `Fp5` digest.
///
/// Splits the bytes into 8-byte little-endian Goldilocks limbs (zero-padding
/// the trailing chunk) and runs [`hash_to_quintic_extension`] over the limbs.
/// Returns the canonical 40-byte little-endian encoding the Schnorr binding
/// signs over.
///
/// # Errors
///
/// Returns [`AuthTokenError::MessageEncoding`] if any 8-byte chunk decodes to
/// a non-canonical Goldilocks element. The auth-token format is ASCII
/// (`'0'..='9'` and `':'`), so every byte sits in `0..=0x3A` and the case is
/// unreachable for tokens emitted by [`auth_token_message`]; the error path
/// exists for callers that pass arbitrary preimages.
pub fn hash_auth_message(message: &str) -> Result<[u8; 40], AuthTokenError> {
    let elems = ascii_to_fp_limbs(message.as_bytes())?;
    Ok(hash_to_quintic_extension(&elems).to_le_bytes())
}

fn sign_message(
    message: &str,
    sk: &PrivateKey,
    k: Scalar,
) -> Result<[u8; SIG_BYTES], AuthTokenError> {
    let elems = ascii_to_fp_limbs(message.as_bytes())?;
    let digest = hash_to_quintic_extension(&elems);
    Ok(sk.sign(digest, k).to_le_bytes())
}

fn ascii_to_fp_limbs(bytes: &[u8]) -> Result<Vec<Fp>, AuthTokenError> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(8));
    let mut i = 0;

    while i < bytes.len() {
        let end = core::cmp::min(i + 8, bytes.len());
        let mut limb = [0u8; 8];
        limb[..end - i].copy_from_slice(&bytes[i..end]);
        let fp =
            Fp::try_from_le_bytes(limb).ok_or(AuthTokenError::MessageEncoding { offset: i })?;
        out.push(fp);
        i = end;
    }

    Ok(out)
}

fn format_token(message: &str, sig: &[u8; SIG_BYTES]) -> String {
    // Lowercase, no `0x` prefix: matches `ethCommon.Bytes2Hex` in the Go
    // reference, which is what the venue's REST/WS handshake expects.
    let mut out = String::with_capacity(message.len() + 1 + SIG_BYTES * 2);
    out.push_str(message);
    out.push(':');
    for b in sig {
        write!(&mut out, "{b:02x}").expect("writing into String never fails");
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::signing::{
        curve::SCALAR_BYTES, field::Fp5, fixtures::hex_to_array, schnorr::Signature,
    };

    fn fixed_sk() -> PrivateKey {
        // Same seed bytes as the tx-oracle fixture; keeps the auth-token
        // tests self-contained without piggybacking on the tx fixture file.
        let bytes: [u8; SCALAR_BYTES] = [
            0x0b, 0x8e, 0x0f, 0x63, 0xc2, 0x4d, 0x8b, 0xaa, 0xcd, 0x9d, 0x29, 0xad, 0x4e, 0x9a,
            0x4b, 0x73, 0xc4, 0xa8, 0xd2, 0xbb, 0x8b, 0x16, 0xdc, 0x4f, 0xa9, 0xd7, 0xc2, 0xe1,
            0xd3, 0xa8, 0xb1, 0xf0, 0xe8, 0xd3, 0xa4, 0xc5, 0xb6, 0xe7, 0xf0, 0x01,
        ];
        PrivateKey::from_le_bytes_reduce(bytes)
    }

    fn nonzero_k() -> Scalar {
        let mut bytes = [0u8; SCALAR_BYTES];
        bytes[0] = 0x42;
        bytes[7] = 0x01;
        Scalar::from_le_bytes_reduce(bytes)
    }

    #[rstest]
    fn message_format_matches_go_reference() {
        let m = auth_token_message(1_777_809_907, 12345, 5);
        assert_eq!(m, "1777809907:12345:5", "was {m}");
    }

    #[rstest]
    fn build_auth_token_smoke_test() {
        // Smoke-test the system-clock variant: seed a deadline 600s ahead of
        // wall-clock now and verify the token's structural shape. The signing
        // pipeline is exercised by the *_at variant; this test just gates the
        // SystemTime plumbing and Result threading.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let now_i64 = i64::try_from(now).unwrap();
        let deadline = now_i64 + 600;
        let account_index = 12345i64;
        let api_key_index = 5u8;
        let token = build_auth_token(
            deadline,
            account_index,
            api_key_index,
            &fixed_sk(),
            nonzero_k(),
        )
        .expect("future deadline must sign");

        let prefix = format!("{deadline}:{account_index}:{api_key_index}:");
        assert!(
            token.starts_with(&prefix),
            "token must start with deadline:account:key:, was {token}",
        );
        let sig_hex = &token[prefix.len()..];
        assert_eq!(
            sig_hex.len(),
            SIG_BYTES * 2,
            "hex sig must span 160 chars, was {}",
            sig_hex.len(),
        );
    }

    #[rstest]
    fn token_is_message_colon_hex_sig() {
        let token =
            build_auth_token_at(1_000_000, 1_000_300, 12345, 5, &fixed_sk(), nonzero_k()).unwrap();
        let mut parts = token.rsplitn(2, ':');
        let sig_hex = parts.next().expect("token must have sig component");
        let prefix = parts.next().expect("token must have message prefix");

        assert_eq!(prefix, "1000300:12345:5", "was {prefix}");
        assert_eq!(
            sig_hex.len(),
            SIG_BYTES * 2,
            "hex sig must span 160 chars, was {}",
            sig_hex.len(),
        );
        assert!(
            sig_hex
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "sig must be lowercase hex, was {sig_hex}",
        );
    }

    #[rstest]
    fn token_signature_verifies_under_derived_pubkey() {
        let sk = fixed_sk();
        let pk = sk.public_key();
        let deadline = 1_000_300;
        let token = build_auth_token_at(1_000_000, deadline, 12345, 5, &sk, nonzero_k()).unwrap();

        let (message, sig_hex) = split_token(&token);
        let digest_bytes = hash_auth_message(&message).expect("ASCII input must hash");
        let digest = Fp5::try_from_le_bytes(digest_bytes).expect("digest must be canonical");
        let sig = decode_sig(&sig_hex);

        assert!(
            pk.verify(digest, &sig),
            "self-issued token must verify under derived pubkey",
        );
    }

    #[rstest]
    fn deadline_in_past_errors() {
        let err = build_auth_token_at(1_000, 999, 1, 0, &fixed_sk(), nonzero_k())
            .expect_err("must reject past deadline");
        assert_eq!(
            err,
            AuthTokenError::DeadlineNotInFuture {
                deadline: 999,
                now: 1_000,
            },
        );
    }

    #[rstest]
    fn deadline_equal_to_now_errors() {
        let err = build_auth_token_at(1_000, 1_000, 1, 0, &fixed_sk(), nonzero_k())
            .expect_err("must reject equal deadline");
        assert_eq!(
            err,
            AuthTokenError::DeadlineNotInFuture {
                deadline: 1_000,
                now: 1_000,
            },
        );
    }

    #[rstest]
    fn deadline_beyond_max_ttl_errors() {
        let now = 1_000_000;
        let max_ttl = i64::try_from(LIGHTER_AUTH_TOKEN_MAX_TTL.as_secs()).unwrap();
        let deadline = now + max_ttl + 1;
        let err = build_auth_token_at(now, deadline, 1, 0, &fixed_sk(), nonzero_k())
            .expect_err("must reject TTL above cap");
        assert_eq!(
            err,
            AuthTokenError::TtlTooLarge {
                deadline,
                now,
                max_ttl_secs: max_ttl,
            },
        );
    }

    #[rstest]
    fn deadline_at_max_ttl_succeeds() {
        let now = 1_000_000;
        let max_ttl = i64::try_from(LIGHTER_AUTH_TOKEN_MAX_TTL.as_secs()).unwrap();
        let deadline = now + max_ttl;
        let token = build_auth_token_at(now, deadline, 1, 0, &fixed_sk(), nonzero_k())
            .expect("max-TTL deadline must sign");
        assert!(
            token.starts_with(&format!("{deadline}:1:0:")),
            "was {token}"
        );
    }

    #[rstest]
    fn hash_input_packs_eight_bytes_per_limb() {
        // "abc" -> single limb [0x61, 0x62, 0x63, 0, 0, 0, 0, 0].
        let elems = super::ascii_to_fp_limbs(b"abc").expect("ASCII input must encode");
        assert_eq!(elems.len(), 1);
        let limb = u64::from_le_bytes([b'a', b'b', b'c', 0, 0, 0, 0, 0]);
        assert_eq!(elems[0].to_u64(), limb);

        // 9 bytes spill into two limbs: first full, second carries one byte.
        let elems = super::ascii_to_fp_limbs(b"abcdefghI").expect("ASCII input must encode");
        assert_eq!(elems.len(), 2);
        assert_eq!(elems[1].to_u64(), u64::from(b'I'));
    }

    #[rstest]
    fn hash_input_rejects_non_canonical_limb() {
        // u64::MAX > MODULUS so the 8-byte chunk is non-canonical.
        let mut bytes = [0xFFu8; 8];
        bytes[7] = 0xFF;
        let err = super::ascii_to_fp_limbs(&bytes).expect_err("must reject non-canonical");
        assert_eq!(err, AuthTokenError::MessageEncoding { offset: 0 });
    }

    fn split_token(token: &str) -> (String, String) {
        let mut parts = token.rsplitn(2, ':');
        let sig_hex = parts.next().unwrap().to_string();
        let message = parts.next().unwrap().to_string();
        (message, sig_hex)
    }

    fn decode_sig(hex: &str) -> Signature {
        assert_eq!(hex.len(), SIG_BYTES * 2);
        let mut buf = [0u8; SIG_BYTES];
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        Signature::from_le_bytes_reduce(buf)
    }

    proptest! {
        /// Any ASCII (high-bit-clear) byte sequence packs into Fp limbs and
        /// unpacks back to the original bytes, zero-padded on the right to a
        /// multiple of 8. ASCII bytes keep each 8-byte chunk's `u64` strictly
        /// below the Goldilocks modulus, so encoding never errors.
        #[rstest]
        fn prop_ascii_to_fp_limbs_round_trip(
            input in proptest::collection::vec(0u8..=0x7F, 0..64),
        ) {
            let limbs = super::ascii_to_fp_limbs(&input).expect("ASCII input must encode");
            prop_assert_eq!(limbs.len(), input.len().div_ceil(8));

            let mut unpacked = Vec::with_capacity(limbs.len() * 8);
            for fp in &limbs {
                unpacked.extend_from_slice(&fp.to_u64().to_le_bytes());
            }

            let mut padded = input;
            while padded.len() % 8 != 0 {
                padded.push(0);
            }
            prop_assert_eq!(unpacked, padded);
        }

        /// `hash_auth_message` is deterministic over arbitrary ASCII inputs.
        #[rstest]
        fn prop_hash_auth_message_deterministic(s in "[ -~]{0,128}") {
            let h1 = super::hash_auth_message(&s).expect("ASCII must hash");
            let h2 = super::hash_auth_message(&s).expect("ASCII must hash");
            prop_assert_eq!(h1, h2);
        }

        /// Self-issued tokens always verify under the derived public key for
        /// any non-zero `k` and any in-range deadline.
        #[rstest]
        fn prop_self_issued_token_verifies(
            account_index in 0i64..1_000_000_000,
            api_key_index in 0u8..=255,
            ttl_secs in 1i64..(LIGHTER_AUTH_TOKEN_MAX_TTL.as_secs() as i64),
            k_seed in 1u64..u64::MAX,
        ) {
            let sk = fixed_sk();
            let pk = sk.public_key();
            let now = 1_700_000_000;
            let deadline = now + ttl_secs;

            let mut k_bytes = [0u8; SCALAR_BYTES];
            k_bytes[..8].copy_from_slice(&k_seed.to_le_bytes());
            let k = Scalar::from_le_bytes_reduce(k_bytes);
            prop_assume!(!k.is_zero());

            let token = build_auth_token_at(
                now, deadline, account_index, api_key_index, &sk, k,
            )
            .unwrap();
            let (message, sig_hex) = split_token(&token);
            let expected = format!("{deadline}:{account_index}:{api_key_index}");
            prop_assert_eq!(&message, &expected);

            let digest_bytes = hash_auth_message(&message).expect("ASCII input must hash");
            let digest = Fp5::try_from_le_bytes(digest_bytes).unwrap();
            let sig = decode_sig(&sig_hex);
            prop_assert!(pk.verify(digest, &sig));
        }
    }

    /// Layer 2 oracle: the closed-source signer's auth tokens must verify
    /// under the same public key our `PrivateKey::sign` derives, against the
    /// same `hash_auth_message` digest.
    #[rstest]
    fn oracle_auth_tokens_verify_against_our_hash() {
        const ORACLE_JSON: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/signing_auth_token_oracle.json",
        ));

        #[derive(serde::Deserialize)]
        struct File {
            vectors: Vec<Vector>,
        }

        #[derive(serde::Deserialize)]
        struct Vector {
            sk: String,
            account_index: i64,
            api_key_index: u8,
            deadline: i64,
            token: String,
        }

        let suite: File = serde_json::from_str(ORACLE_JSON).expect("parse oracle");
        assert!(!suite.vectors.is_empty(), "oracle vectors empty");

        for (i, v) in suite.vectors.iter().enumerate() {
            let sk_bytes = hex_to_array::<SCALAR_BYTES>(&v.sk);
            let sk = PrivateKey::from_le_bytes_reduce(sk_bytes);
            let pk = sk.public_key();

            let expected_message = auth_token_message(v.deadline, v.account_index, v.api_key_index);
            let (message, sig_hex) = split_token(&v.token);
            assert_eq!(
                message, expected_message,
                "vector {i}: token prefix diverged, was {message}",
            );

            let digest_bytes = hash_auth_message(&message).expect("oracle message must hash");
            let digest =
                Fp5::try_from_le_bytes(digest_bytes).expect("auth-token digest must be canonical");
            let sig = decode_sig(&sig_hex);
            assert!(
                pk.verify(digest, &sig),
                "vector {i}: oracle sig must verify against our recomputed digest",
            );
        }
    }

    #[rstest]
    fn fresh_k_returns_canonical_scalar() {
        // The CSPRNG draw is reduced modulo the curve order, so every call
        // returns a canonical scalar regardless of the raw byte values.
        for _ in 0..16 {
            let k = fresh_k();
            assert!(
                k.is_canonical(),
                "fresh_k must return a canonical scalar, was {k:?}",
            );
        }
    }

    #[rstest]
    fn fresh_k_yields_distinct_scalars() {
        // With 320 bits of entropy a collision in three draws is unreachable
        // in any realistic execution; this guards against a hard-coded
        // constant or a misconfigured RNG.
        let a = fresh_k();
        let b = fresh_k();
        let c = fresh_k();
        assert!(
            !(a == b && b == c),
            "fresh_k must vary across calls (a={a:?}, b={b:?}, c={c:?})",
        );
    }

    #[rstest]
    fn build_auth_token_for_round_trips_against_credential() {
        // Mint a token for the credential and verify the embedded signature
        // against the credential's public key. End-to-end check that the
        // helper threads private_key, account_index, and api_key_index
        // through the message and signature correctly.
        const PRIVATE_KEY_HEX: &str =
            "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";
        let credential = crate::common::credential::Credential::new(5, PRIVATE_KEY_HEX, 12_345)
            .expect("credential must construct");

        let token = build_auth_token_for(&credential).expect("token mint must succeed");

        let pk = credential.private_key().unwrap().public_key();
        let (message, sig_hex) = token
            .rsplit_once(':')
            .expect("token must end with `:hex(sig)`");
        let digest_bytes = hash_auth_message(message).expect("hash must succeed");
        let digest = Fp5::try_from_le_bytes(digest_bytes).expect("digest must be canonical");
        let sig_bytes = hex_to_array::<{ SIG_BYTES }>(sig_hex);
        let sig = Signature::from_le_bytes_reduce(sig_bytes);
        assert!(
            pk.verify(digest, &sig),
            "minted token must verify against credential public key",
        );

        // Sanity check that the message body is shaped `deadline:account:api_key`.
        let parts: Vec<&str> = message.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1], "12345");
        assert_eq!(parts[2], "5");
    }
}
