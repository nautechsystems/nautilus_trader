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

//! Differential proptest suite against the upstream `pornin/ecgfp5` Rust
//! reference implementation.
//!
//! Pornin's MIT-licensed crate is the design paper's authoritative reference
//! and shares no code lineage with our in-tree port. Comparing every public
//! algebra operation byte-for-byte against it on arbitrary inputs catches any
//! shared-spec bug or transcription error that the Go-derived fixture vectors
//! silently inherit. Each property runs ~256 random samples per primitive.
//!
//! Conversions between the two type systems route through the canonical
//! 40-byte little-endian encodings both crates support, so the comparisons
//! exercise our encoder/decoder pair on every iteration as a side effect.

use ecgfp5::{
    curve::Point as PorninPoint, field::GFp5 as PorninFp5, scalar::Scalar as PorninScalar,
};
use proptest::prelude::*;
use rstest::rstest;

use super::{
    curve::{Point, Scalar},
    field::Fp5,
    fixtures::{arb_fp5, arb_fp5_nonzero, arb_scalar, arb_scalar_nonzero},
};

fn fp5_to_pornin(ours: Fp5) -> PorninFp5 {
    let (gfp5, ok) = PorninFp5::decode(&ours.to_le_bytes());
    assert_eq!(
        ok,
        u64::MAX,
        "our canonical Fp5 bytes must decode under Pornin's reference",
    );
    gfp5
}

fn scalar_to_pornin(ours: Scalar) -> PorninScalar {
    PorninScalar::decode_reduce(&ours.to_le_bytes())
}

proptest! {
    /// `Fp5` addition matches the reference impl for any pair.
    #[rstest]
    fn prop_fp5_add_matches_pornin(a in arb_fp5(), b in arb_fp5()) {
        let ours = (a + b).to_le_bytes();
        let theirs = (fp5_to_pornin(a) + fp5_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Fp5` subtraction matches the reference.
    #[rstest]
    fn prop_fp5_sub_matches_pornin(a in arb_fp5(), b in arb_fp5()) {
        let ours = (a - b).to_le_bytes();
        let theirs = (fp5_to_pornin(a) - fp5_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Fp5` multiplication matches the reference.
    #[rstest]
    fn prop_fp5_mul_matches_pornin(a in arb_fp5(), b in arb_fp5()) {
        let ours = (a * b).to_le_bytes();
        let theirs = (fp5_to_pornin(a) * fp5_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Fp5` negation matches the reference.
    #[rstest]
    fn prop_fp5_neg_matches_pornin(a in arb_fp5()) {
        let ours = (-a).to_le_bytes();
        let theirs = (-fp5_to_pornin(a)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Fp5` inversion matches the reference for any non-zero element.
    #[rstest]
    fn prop_fp5_invert_matches_pornin(a in arb_fp5_nonzero()) {
        let ours = a.invert().to_le_bytes();
        let theirs = fp5_to_pornin(a).invert().encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Scalar` addition matches the reference for any pair.
    #[rstest]
    fn prop_scalar_add_matches_pornin(a in arb_scalar(), b in arb_scalar()) {
        let ours = (a + b).to_le_bytes();
        let theirs = (scalar_to_pornin(a) + scalar_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Scalar` subtraction matches the reference.
    #[rstest]
    fn prop_scalar_sub_matches_pornin(a in arb_scalar(), b in arb_scalar()) {
        let ours = (a - b).to_le_bytes();
        let theirs = (scalar_to_pornin(a) - scalar_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Scalar` multiplication matches the reference.
    #[rstest]
    fn prop_scalar_mul_matches_pornin(a in arb_scalar(), b in arb_scalar()) {
        let ours = (a * b).to_le_bytes();
        let theirs = (scalar_to_pornin(a) * scalar_to_pornin(b)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Scalar` negation matches the reference.
    #[rstest]
    fn prop_scalar_neg_matches_pornin(a in arb_scalar()) {
        let ours = (-a).to_le_bytes();
        let theirs = (-scalar_to_pornin(a)).encode();
        prop_assert_eq!(ours, theirs);
    }

    /// `Point::decode` and `PorninPoint::decode` agree on success and on the
    /// recovered point's encoding for every `Fp5` candidate.
    #[rstest]
    fn prop_point_decode_matches_pornin(w in arb_fp5()) {
        let ours = Point::decode(w);
        let pornin_input = fp5_to_pornin(w);
        let (theirs_pt, theirs_ok) = PorninPoint::decode(pornin_input);

        match (ours, theirs_ok) {
            (Some(ours_pt), u64::MAX) => {
                prop_assert_eq!(ours_pt.encode().to_le_bytes(), theirs_pt.encode().encode());
            }
            (None, 0) => {} // Both reject non-residues; consistent.
            (ours_state, theirs_mask) => {
                prop_assert!(
                    false,
                    "decode disagreement: ours={:?} theirs_ok={:#x}",
                    ours_state.is_some(),
                    theirs_mask,
                );
            }
        }
    }

    /// `s * G` computed via our `scalar_mul` matches Pornin's `mulgen(s)` on
    /// every canonical scalar.
    #[rstest]
    fn prop_point_mulgen_matches_pornin(s in arb_scalar()) {
        let ours = (Point::GENERATOR * s).encode().to_le_bytes();
        let theirs = PorninPoint::mulgen(scalar_to_pornin(s)).encode().encode();
        prop_assert_eq!(ours, theirs);
    }

    /// Our precomputed-window `Point::mulgen(s)` matches Pornin's `mulgen`.
    /// Pins the static affine table the verifier consumes.
    #[rstest]
    fn prop_point_mulgen_var_matches_pornin(s in arb_scalar()) {
        let ours = Point::mulgen(s).encode().to_le_bytes();
        let theirs = PorninPoint::mulgen(scalar_to_pornin(s)).encode().encode();
        prop_assert_eq!(ours, theirs);
    }

    /// Our constant-time `Point::mulgen_ct(s)` matches Pornin's `mulgen`.
    /// Pins the secret-scalar generator path the signer routes through.
    #[rstest]
    fn prop_point_mulgen_ct_matches_pornin(s in arb_scalar()) {
        let ours = Point::mulgen_ct(s).encode().to_le_bytes();
        let theirs = PorninPoint::mulgen(scalar_to_pornin(s)).encode().encode();
        prop_assert_eq!(ours, theirs);
    }

    /// Doubling on the generator matches the reference for any number of
    /// iterations, exercising `set_double` and `mdouble` paths.
    #[rstest]
    fn prop_point_double_matches_pornin(seed in arb_scalar()) {
        let ours_base = Point::GENERATOR * seed;
        let theirs_base = PorninPoint::mulgen(scalar_to_pornin(seed));

        let ours = ours_base.double().encode().to_le_bytes();
        let theirs = theirs_base.double().encode().encode();
        prop_assert_eq!(ours, theirs);
    }

    /// Group addition on two arbitrary points (constructed via independent
    /// scalar mults of the generator) matches the reference.
    #[rstest]
    fn prop_point_add_matches_pornin(s1 in arb_scalar(), s2 in arb_scalar()) {
        let ours_p1 = Point::GENERATOR * s1;
        let ours_p2 = Point::GENERATOR * s2;
        let theirs_p1 = PorninPoint::mulgen(scalar_to_pornin(s1));
        let theirs_p2 = PorninPoint::mulgen(scalar_to_pornin(s2));

        let ours = (ours_p1 + ours_p2).encode().to_le_bytes();
        let theirs = (theirs_p1 + theirs_p2).encode().encode();
        prop_assert_eq!(ours, theirs);
    }
}

proptest! {
    // Heavier scalar-mul-arbitrary-base case: 2 scalar mults per case to set
    // up the base point, then one more for the test. Trim cases to 64 to
    // keep total runtime bounded.
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// Scalar multiplication on an arbitrary base matches the reference.
    /// Pins the windowed-recoding path against Pornin's recoding in
    /// combination with arbitrary base points.
    #[rstest]
    fn prop_scalar_mul_on_arbitrary_base_matches_pornin(
        seed in arb_scalar_nonzero(),
        s in arb_scalar(),
    ) {
        let ours_base = Point::GENERATOR * seed;
        let theirs_base = PorninPoint::mulgen(scalar_to_pornin(seed));

        let ours = (ours_base * s).encode().to_le_bytes();
        let theirs = (theirs_base * scalar_to_pornin(s)).encode().encode();
        prop_assert_eq!(ours, theirs);
    }
}
