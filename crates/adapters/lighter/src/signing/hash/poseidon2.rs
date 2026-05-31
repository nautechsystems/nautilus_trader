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

//! Poseidon2 permutation and sponge over the Goldilocks field `Fp`.
//!
//! The permutation operates on a fixed-width state of [`WIDTH`] field elements
//! and is composed of [`ROUNDS_F_HALF`] external rounds, then `ROUNDS_P`
//! internal rounds with the S-box restricted to position 0, then a final
//! [`ROUNDS_F_HALF`] external rounds. An initial external linear layer is
//! applied before any round constants are added (the Poseidon2 specification's
//! "pre-mix"). All round constants and the diagonal of the internal MDS matrix
//! live in [`super::params`].
//!
//! The sponge is the "overwrite" variant the Lighter reference uses: each
//! absorption block writes the input block into the leading [`RATE`] state
//! positions (without XORing into prior state) and then permutes; the capacity
//! `WIDTH - RATE` positions are never touched by absorption. Squeezing reads
//! [`RATE`] elements at a time and re-permutes on demand. No length padding is
//! applied; callers are responsible for domain separation.
//!
//! All arithmetic is performed through the public [`Fp`] API, so the
//! constant-time guarantees of the field carry over without exception. The
//! permutation contains no data-dependent branches; the sponge has only the
//! public-information branches on input length and requested output length.

use super::params::{
    EXTERNAL_CONSTANTS, INTERNAL_CONSTANTS, MATRIX_DIAG_12, RATE, ROUNDS_F_HALF, WIDTH,
};
use crate::signing::field::{Fp, Fp5};

/// Output digest of the standard Poseidon2 hash, holding [`HASH_OUT`] field elements.
pub const HASH_OUT: usize = 4;

/// In-place Poseidon2 permutation over a [`WIDTH`]-element state.
pub fn permute(state: &mut [Fp; WIDTH]) {
    external_linear_layer(state);
    full_rounds(state, 0);
    partial_rounds(state);
    full_rounds(state, ROUNDS_F_HALF);
}

/// Run the absorption phase of the "overwrite, no-pad" sponge over `input`
/// and return the resulting state. Callers squeeze by reading the leading
/// state positions directly (`num_outputs <= RATE` paths) or by iterating
/// `permute` between reads ([`hash_n_to_m_no_pad`]).
///
/// With an empty `input`, the state stays at zero and no permute is run.
#[inline]
fn absorb(input: &[Fp]) -> [Fp; WIDTH] {
    let mut state = [Fp::ZERO; WIDTH];

    let mut i = 0;
    while i < input.len() {
        let chunk_end = core::cmp::min(i + RATE, input.len());
        for (j, &val) in input[i..chunk_end].iter().enumerate() {
            state[j] = val;
        }
        permute(&mut state);
        i += RATE;
    }

    state
}

/// Variable-length absorb / variable-length squeeze sponge built on [`permute`].
///
/// Returns `num_outputs` field elements derived from `input` under the Lighter
/// "overwrite, no-pad" sponge convention. With an empty `input`, the state
/// stays at zero and the squeeze reads zeros directly without permuting.
///
/// Fixed-size callers ([`hash_n_to_hash_no_pad`], [`hash_to_quintic_extension`])
/// bypass the [`Vec`] allocation; this entry point exists for callers that
/// need a variable `num_outputs`.
#[must_use]
pub fn hash_n_to_m_no_pad(input: &[Fp], num_outputs: usize) -> Vec<Fp> {
    let mut state = absorb(input);

    let mut out = Vec::with_capacity(num_outputs);
    while out.len() < num_outputs {
        for slot in &state[..RATE] {
            out.push(*slot);
            if out.len() == num_outputs {
                return out;
            }
        }
        permute(&mut state);
    }
    out
}

/// Compress an arbitrary-length input to a fixed [`HASH_OUT`]-element digest.
///
/// `HASH_OUT <= RATE`, so the squeeze never re-permutes and there is no
/// reason to allocate a [`Vec`].
#[must_use]
pub fn hash_n_to_hash_no_pad(input: &[Fp]) -> [Fp; HASH_OUT] {
    let state = absorb(input);
    [state[0], state[1], state[2], state[3]]
}

/// Convenience alias for [`hash_n_to_hash_no_pad`] matching the Lighter Go API.
#[must_use]
pub fn hash_no_pad(input: &[Fp]) -> [Fp; HASH_OUT] {
    hash_n_to_hash_no_pad(input)
}

/// Two-to-one compression of two [`HASH_OUT`]-element digests.
#[must_use]
pub fn hash_two_to_one(a: [Fp; HASH_OUT], b: [Fp; HASH_OUT]) -> [Fp; HASH_OUT] {
    let buf = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
    hash_n_to_hash_no_pad(&buf)
}

/// Iteratively compress `inputs` left-to-right via [`hash_two_to_one`].
///
/// Returns `inputs[0]` when the slice has a single element, mirroring the Go
/// reference's `HashNToOne`.
///
/// # Panics
///
/// Panics if `inputs` is empty.
#[must_use]
pub fn hash_n_to_one(inputs: &[[Fp; HASH_OUT]]) -> [Fp; HASH_OUT] {
    assert!(
        !inputs.is_empty(),
        "hash_n_to_one requires at least one input"
    );

    if inputs.len() == 1 {
        return inputs[0];
    }

    let mut acc = hash_two_to_one(inputs[0], inputs[1]);
    for next in &inputs[2..] {
        acc = hash_two_to_one(acc, *next);
    }
    acc
}

/// Hash an arbitrary-length input into the quintic extension `Fp5`.
///
/// Squeezes 5 field elements and packs them into an [`Fp5`] limb-wise. Used by
/// the Lighter Schnorr binding to derive a curve scalar from a message digest.
///
/// `5 <= RATE`, so the squeeze never re-permutes and there is no reason to
/// allocate a [`Vec`].
#[must_use]
pub fn hash_to_quintic_extension(input: &[Fp]) -> Fp5 {
    let state = absorb(input);
    Fp5([state[0], state[1], state[2], state[3], state[4]])
}

/// Hash a `(Fp5, Fp5)` pair as a 10-element preimage into a single `Fp5`.
///
/// Concatenates `a.0 || b.0` and feeds the result through
/// [`hash_to_quintic_extension`]. Used by Schnorr signing/verification (where
/// the pair is `(r, hashed_msg)`) and by the L2 tx aggregation step (where the
/// pair is `(body_digest, attribute_digest)`).
#[must_use]
pub fn hash_two_to_quintic(a: Fp5, b: Fp5) -> Fp5 {
    let mut preimage = [Fp::ZERO; 10];
    preimage[..5].copy_from_slice(&a.0);
    preimage[5..].copy_from_slice(&b.0);
    hash_to_quintic_extension(&preimage)
}

/// One full (external) S-box layer: `state[i] <- state[i]^7` for all `i`.
fn sbox_full(state: &mut [Fp; WIDTH]) {
    for slot in state.iter_mut() {
        *slot = sbox(*slot);
    }
}

/// S-box on a single element: `x -> x^7`.
#[inline]
fn sbox(x: Fp) -> Fp {
    let x2 = x.square();
    let x6 = (x2 * x).square();
    x6 * x
}

/// External linear layer: composition of a 4x4 MDS on each disjoint block of 4
/// state positions with the all-ones lift across the three blocks. Matches the
/// Plonky3 / Lighter circulant `circ(2, 3, 1, 1)` formulation.
fn external_linear_layer(state: &mut [Fp; WIDTH]) {
    for block in 0..3 {
        let base = block * 4;
        let s0 = state[base];
        let s1 = state[base + 1];
        let s2 = state[base + 2];
        let s3 = state[base + 3];
        let t0 = s0 + s1;
        let t1 = s2 + s3;
        let t2 = t0 + t1;
        let t3 = t2 + s1;
        let t4 = t2 + s3;
        let t5 = s0 + s0;
        let t6 = s2 + s2;
        state[base] = t3 + t0;
        state[base + 1] = t6 + t3;
        state[base + 2] = t1 + t4;
        state[base + 3] = t5 + t4;
    }

    let mut sums = [Fp::ZERO; 4];

    for k in 0..4 {
        let mut j = 0;

        while j < WIDTH {
            sums[k] += state[j + k];
            j += 4;
        }
    }

    for i in 0..WIDTH {
        state[i] += sums[i % 4];
    }
}

/// Internal linear layer: `state <- (diag(MATRIX_DIAG_12) + J) * state`,
/// where `J` is the all-ones matrix.
fn internal_linear_layer(state: &mut [Fp; WIDTH]) {
    let mut sum = state[0];

    for slot in &state[1..] {
        sum += *slot;
    }

    for i in 0..WIDTH {
        state[i] = state[i] * MATRIX_DIAG_12[i] + sum;
    }
}

fn full_rounds(state: &mut [Fp; WIDTH], start: usize) {
    for round_consts in &EXTERNAL_CONSTANTS[start..start + ROUNDS_F_HALF] {
        for (slot, rc) in state.iter_mut().zip(round_consts.iter()) {
            *slot += *rc;
        }
        sbox_full(state);
        external_linear_layer(state);
    }
}

fn partial_rounds(state: &mut [Fp; WIDTH]) {
    for rc in &INTERNAL_CONSTANTS {
        state[0] += *rc;
        state[0] = sbox(state[0]);
        internal_linear_layer(state);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;
    use crate::signing::fixtures::{arb_fp, bytes_to_hex, hex_to_bytes};

    const VECTORS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_hash_poseidon2_vectors.json",
    ));

    #[derive(Debug, Deserialize)]
    struct VectorsFile {
        vectors: Vectors,
    }

    #[derive(Debug, Deserialize)]
    struct Vectors {
        permute: Vec<PermuteVector>,
        sponge: Vec<SpongeVector>,
        hash_to_quintic: Vec<QuinticVector>,
        hash_n_to_one: Vec<HashNToOneVector>,
    }

    #[derive(Debug, Deserialize)]
    struct PermuteVector {
        input: String,
        output: String,
    }

    #[derive(Debug, Deserialize)]
    struct SpongeVector {
        input: String,
        num_outputs: usize,
        output: String,
    }

    #[derive(Debug, Deserialize)]
    struct QuinticVector {
        input: String,
        output: String,
    }

    #[derive(Debug, Deserialize)]
    struct HashNToOneVector {
        inputs: Vec<String>,
        output: String,
    }

    fn decode_fps(hex: &str) -> Vec<Fp> {
        let bytes = hex_to_bytes(hex);
        assert!(
            bytes.len().is_multiple_of(8),
            "fp encoding must be 8-byte multiples, was {} bytes",
            bytes.len(),
        );
        bytes
            .chunks_exact(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(chunk);
                Fp::try_from_le_bytes(buf).expect("non-canonical Fp limb")
            })
            .collect()
    }

    fn encode_fps(fs: &[Fp]) -> String {
        let mut bytes = Vec::with_capacity(fs.len() * 8);
        for f in fs {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes_to_hex(&bytes)
    }

    #[rstest]
    fn permute_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.permute.is_empty(), "permute vectors empty");

        for (i, v) in suite.vectors.permute.iter().enumerate() {
            let input = decode_fps(&v.input);
            assert_eq!(input.len(), WIDTH, "vector {i}: input width");

            let mut state = [Fp::ZERO; WIDTH];
            state.copy_from_slice(&input);
            permute(&mut state);

            assert_eq!(encode_fps(&state), v.output, "vector {i}: permute output");
        }
    }

    #[rstest]
    fn sponge_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.sponge.is_empty(), "sponge vectors empty");

        for (i, v) in suite.vectors.sponge.iter().enumerate() {
            let input = decode_fps(&v.input);
            let out = hash_n_to_m_no_pad(&input, v.num_outputs);

            assert_eq!(out.len(), v.num_outputs, "vector {i}: sponge output length");
            assert_eq!(encode_fps(&out), v.output, "vector {i}: sponge output");
        }
    }

    #[rstest]
    fn hash_to_quintic_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(
            !suite.vectors.hash_to_quintic.is_empty(),
            "hash_to_quintic vectors empty",
        );

        for (i, v) in suite.vectors.hash_to_quintic.iter().enumerate() {
            let input = decode_fps(&v.input);
            let out = hash_to_quintic_extension(&input);

            assert_eq!(
                bytes_to_hex(&out.to_le_bytes()),
                v.output,
                "vector {i}: hash_to_quintic output",
            );
        }
    }

    #[rstest]
    fn hash_two_to_one_matches_concatenation() {
        let a = [
            Fp::from_u64_reduce(1),
            Fp::from_u64_reduce(2),
            Fp::from_u64_reduce(3),
            Fp::from_u64_reduce(4),
        ];
        let b = [
            Fp::from_u64_reduce(5),
            Fp::from_u64_reduce(6),
            Fp::from_u64_reduce(7),
            Fp::from_u64_reduce(8),
        ];
        let buf = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
        assert_eq!(hash_two_to_one(a, b), hash_n_to_hash_no_pad(&buf));
    }

    #[rstest]
    fn hash_n_to_one_single_input_is_identity() {
        let a = [
            Fp::from_u64_reduce(11),
            Fp::from_u64_reduce(22),
            Fp::from_u64_reduce(33),
            Fp::from_u64_reduce(44),
        ];
        assert_eq!(hash_n_to_one(&[a]), a);
    }

    #[rstest]
    fn hash_n_to_one_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(
            !suite.vectors.hash_n_to_one.is_empty(),
            "hash_n_to_one vectors empty",
        );

        for (i, v) in suite.vectors.hash_n_to_one.iter().enumerate() {
            let inputs: Vec<[Fp; HASH_OUT]> = v
                .inputs
                .iter()
                .map(|hex| {
                    let limbs = decode_fps(hex);
                    assert_eq!(
                        limbs.len(),
                        HASH_OUT,
                        "vector {i}: each input must encode {HASH_OUT} limbs, was {}",
                        limbs.len(),
                    );
                    [limbs[0], limbs[1], limbs[2], limbs[3]]
                })
                .collect();

            let out = hash_n_to_one(&inputs);

            assert_eq!(
                encode_fps(&out),
                v.output,
                "vector {i}: hash_n_to_one output (n={})",
                inputs.len(),
            );
        }
    }

    #[rstest]
    #[should_panic(expected = "hash_n_to_one requires at least one input")]
    fn hash_n_to_one_empty_panics() {
        let _ = hash_n_to_one(&[]);
    }

    /// Empty input reads zeros from the uninitialised state for the first
    /// `RATE` squeeze outputs (no permute happens before the first wrap of
    /// the squeeze loop). Once `num_outputs > RATE`, the sponge permutes
    /// the all-zero state and subsequent outputs are no longer zero.
    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(RATE - 1)]
    #[case(RATE)]
    fn empty_input_squeezes_zeros_up_to_rate(#[case] num_outputs: usize) {
        let out = hash_n_to_m_no_pad(&[], num_outputs);
        assert_eq!(out.len(), num_outputs, "output length mismatch");
        for (i, fp) in out.iter().enumerate() {
            assert!(fp.is_zero(), "slot {i} must be zero, was {fp:?}");
        }
    }

    /// Sponge runs cleanly across every absorb-loop boundary width.
    /// Verifies no panics, correct output length, and determinism on the
    /// boundary inputs.
    #[rstest]
    #[case(1)]
    #[case(RATE - 1)]
    #[case(RATE)]
    #[case(RATE + 1)]
    #[case(2 * RATE - 1)]
    #[case(2 * RATE)]
    #[case(2 * RATE + 1)]
    #[case(3 * RATE)]
    fn sponge_handles_input_length_at_rate_boundaries(#[case] input_len: usize) {
        let input: Vec<Fp> = (0..input_len)
            .map(|i| Fp::from_u64_reduce(i as u64 + 1))
            .collect();
        let out_a = hash_n_to_m_no_pad(&input, HASH_OUT);
        let out_b = hash_n_to_m_no_pad(&input, HASH_OUT);
        assert_eq!(out_a.len(), HASH_OUT, "input_len {input_len}: length");
        assert_eq!(out_a, out_b, "input_len {input_len}: not deterministic");
    }

    proptest! {
        /// `permute` is deterministic.
        #[rstest]
        fn prop_permute_deterministic(s in any::<[u64; WIDTH]>()) {
            let state: [Fp; WIDTH] = core::array::from_fn(|i| Fp::from_u64_reduce(s[i]));
            let mut s1 = state;
            let mut s2 = state;
            permute(&mut s1);
            permute(&mut s2);
            prop_assert_eq!(s1, s2);
        }

        /// `permute` is injective on distinct states (probabilistic — over
        /// any pair of distinct inputs, outputs almost surely differ).
        #[rstest]
        fn prop_permute_injective_on_pairs(
            s1 in any::<[u64; WIDTH]>(),
            s2 in any::<[u64; WIDTH]>(),
        ) {
            let mut state1: [Fp; WIDTH] = core::array::from_fn(|i| Fp::from_u64_reduce(s1[i]));
            let mut state2: [Fp; WIDTH] = core::array::from_fn(|i| Fp::from_u64_reduce(s2[i]));
            prop_assume!(state1 != state2);
            permute(&mut state1);
            permute(&mut state2);
            prop_assert_ne!(state1, state2);
        }

        /// `hash_no_pad` is deterministic over arbitrary input vectors.
        #[rstest]
        fn prop_hash_no_pad_deterministic(
            input in proptest::collection::vec(arb_fp(), 0..32),
        ) {
            prop_assert_eq!(hash_no_pad(&input), hash_no_pad(&input));
        }

        /// `hash_two_to_one(a, b) == hash_no_pad(a || b)`.
        #[rstest]
        fn prop_hash_two_to_one_equals_concat(
            a in any::<[u64; HASH_OUT]>(),
            b in any::<[u64; HASH_OUT]>(),
        ) {
            let a_fp: [Fp; HASH_OUT] = core::array::from_fn(|i| Fp::from_u64_reduce(a[i]));
            let b_fp: [Fp; HASH_OUT] = core::array::from_fn(|i| Fp::from_u64_reduce(b[i]));
            let concat = [
                a_fp[0], a_fp[1], a_fp[2], a_fp[3],
                b_fp[0], b_fp[1], b_fp[2], b_fp[3],
            ];
            prop_assert_eq!(hash_two_to_one(a_fp, b_fp), hash_no_pad(&concat));
        }

        /// `hash_n_to_one` is the left fold of `hash_two_to_one` for any
        /// non-empty input list.
        #[rstest]
        fn prop_hash_n_to_one_left_fold(
            inputs in proptest::collection::vec(any::<[u64; HASH_OUT]>(), 1..6),
        ) {
            let inputs_fp: Vec<[Fp; HASH_OUT]> = inputs
                .iter()
                .map(|raw| core::array::from_fn(|j| Fp::from_u64_reduce(raw[j])))
                .collect();
            let mut expected = inputs_fp[0];
            for next in &inputs_fp[1..] {
                expected = hash_two_to_one(expected, *next);
            }
            prop_assert_eq!(hash_n_to_one(&inputs_fp), expected);
        }

        /// `hash_to_quintic_extension(input)` packs into Fp5 deterministically.
        #[rstest]
        fn prop_hash_to_quintic_extension_deterministic(
            input in proptest::collection::vec(arb_fp(), 0..32),
        ) {
            prop_assert_eq!(
                hash_to_quintic_extension(&input),
                hash_to_quintic_extension(&input),
            );
        }

        /// `hash_two_to_quintic` matches the explicit 10-element preimage
        /// hash through `hash_to_quintic_extension`.
        #[rstest]
        fn prop_hash_two_to_quintic_matches_concat(
            a in any::<[u64; 5]>(),
            b in any::<[u64; 5]>(),
        ) {
            let a_fp5 = Fp5::from_u64s_reduce(a);
            let b_fp5 = Fp5::from_u64s_reduce(b);
            let mut concat = [Fp::ZERO; 10];
            concat[..5].copy_from_slice(&a_fp5.0);
            concat[5..].copy_from_slice(&b_fp5.0);
            prop_assert_eq!(
                hash_two_to_quintic(a_fp5, b_fp5),
                hash_to_quintic_extension(&concat),
            );
        }
    }
}
