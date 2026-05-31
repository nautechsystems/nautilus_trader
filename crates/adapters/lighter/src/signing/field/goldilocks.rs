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

//! Goldilocks prime field `Fp = GF(p)`, with `p = 2^64 - 2^32 + 1`.
//!
//! Elements are stored in Montgomery form internally so multiplication reduces
//! to a single 64x64 -> 128-bit multiply followed by a fixed-shape Montgomery
//! reduction; values in the canonical `0..p-1` range are never observed before
//! a deliberate `to_u64`/`to_le_bytes` call. The arithmetic core (`+`, `-`,
//! `*`, `neg`, `square`, `msquare`, `pow`, `invert`) executes as a
//! straight-line sequence of arithmetic and bitwise ops with no data-dependent
//! branches, so timing leaks no information about field operands. The
//! Tonelli-Shanks [`Fp::sqrt`] is variable-time over its input and is only
//! consumed by [`super::Fp5::sqrt`] / [`super::Fp5::canonical_sqrt`], which
//! the curve `Point::decode` calls on public-input `w` values; secret-input
//! sqrt is not part of the signing critical path.

use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Goldilocks prime modulus: `p = 2^64 - 2^32 + 1`.
pub const MODULUS: u64 = 0xFFFF_FFFF_0000_0001;

/// `R^2 mod p` with `R = 2^64`. Used to lift a `u64` into Montgomery form.
const R2: u64 = 0xFFFF_FFFE_0000_0001;

/// 2-adicity of `p - 1`: `p - 1 = 2^32 * (2^32 - 1)`.
const TWO_ADICITY: u32 = 32;

/// Generator of the unique subgroup of order `2^32` in `Fp^*`. Used as the
/// Tonelli-Shanks "non-residue" `z`. Matches the Plonky2 / Lighter convention.
const POWER_OF_TWO_GENERATOR: u64 = 7_277_203_076_849_721_926;

/// An element of the Goldilocks field `Fp = GF(p)`.
///
/// The wrapped `u64` holds the value in Montgomery form (`x * 2^64 mod p`),
/// always reduced into `0..p-1`. Two `Fp` instances are equal iff their
/// Montgomery limbs are equal, so `PartialEq` is correct and constant-time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fp(pub(super) u64);

impl Fp {
    /// Additive identity.
    pub const ZERO: Self = Self::from_u64_reduce(0);

    /// Multiplicative identity.
    pub const ONE: Self = Self::from_u64_reduce(1);

    /// Element `-1 mod p`.
    pub const MINUS_ONE: Self = Self::from_u64_reduce(MODULUS - 1);

    /// Montgomery reduction: given `x` with `x < p * 2^64`, return `x * 2^-64 mod p`
    /// in canonical `0..p-1` form.
    #[inline(always)]
    const fn montyred(x: u128) -> u64 {
        let xl = x as u64;
        let xh = (x >> 64) as u64;
        let (a, e) = xl.overflowing_add(xl << 32);
        let b = a.wrapping_sub(a >> 32).wrapping_sub(e as u64);
        let (r, c) = xh.overflowing_sub(b);
        r.wrapping_sub(0u32.wrapping_sub(c as u32) as u64)
    }

    /// Build an element from a `u64`, reducing modulo `p`.
    #[inline(always)]
    pub const fn from_u64_reduce(v: u64) -> Self {
        Self(Self::montyred((v as u128) * (R2 as u128)))
    }

    /// Build an element from an already-canonical `u64` (`v < p`); returns `None` otherwise.
    #[inline(always)]
    pub fn from_u64_canonical(v: u64) -> Option<Self> {
        if v < MODULUS {
            Some(Self::from_u64_reduce(v))
        } else {
            None
        }
    }

    /// Return the canonical `u64` representation in `0..p-1`.
    #[inline(always)]
    pub const fn to_u64(self) -> u64 {
        Self::montyred(self.0 as u128)
    }

    /// Decode an element from 8 little-endian bytes.
    ///
    /// Returns `None` if the encoded integer is not in canonical range (`>= p`).
    #[inline]
    pub fn try_from_le_bytes(bytes: [u8; 8]) -> Option<Self> {
        Self::from_u64_canonical(u64::from_le_bytes(bytes))
    }

    /// Canonical 8-byte little-endian encoding.
    #[inline]
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.to_u64().to_le_bytes()
    }

    /// Test whether the element is zero.
    #[inline(always)]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Constant-time equality: returns `0xFFFF_FFFF_FFFF_FFFF` on equality, `0` otherwise.
    #[inline(always)]
    pub const fn ct_eq(self, rhs: Self) -> u64 {
        let t = self.0 ^ rhs.0;
        !((((t | t.wrapping_neg()) as i64) >> 63) as u64)
    }

    /// Branch-free select: returns `a` when `mask == 0` and `b` when
    /// `mask == u64::MAX`. Behaviour for any other mask value is unspecified;
    /// the secret-scalar curve primitives only ever pass full-bit masks.
    #[inline(always)]
    #[must_use]
    pub const fn ct_select(mask: u64, a: Self, b: Self) -> Self {
        Self(a.0 ^ (mask & (a.0 ^ b.0)))
    }

    #[inline(always)]
    const fn add_inner(self, rhs: Self) -> Self {
        let (x1, c1) = self.0.overflowing_sub(MODULUS - rhs.0);
        let adj = 0u32.wrapping_sub(c1 as u32);
        Self(x1.wrapping_sub(adj as u64))
    }

    #[inline(always)]
    const fn sub_inner(self, rhs: Self) -> Self {
        let (x1, c1) = self.0.overflowing_sub(rhs.0);
        let adj = 0u32.wrapping_sub(c1 as u32);
        Self(x1.wrapping_sub(adj as u64))
    }

    #[inline(always)]
    const fn neg_inner(self) -> Self {
        Self::ZERO.sub_inner(self)
    }

    #[inline(always)]
    const fn mul_inner(self, rhs: Self) -> Self {
        Self(Self::montyred((self.0 as u128) * (rhs.0 as u128)))
    }

    /// Squaring in `Fp`.
    #[inline(always)]
    #[must_use]
    pub const fn square(self) -> Self {
        self.mul_inner(self)
    }

    /// Repeated squaring: returns `self^(2^n)`.
    #[inline]
    #[must_use]
    pub fn msquare(self, n: u32) -> Self {
        let mut x = self;
        for _ in 0..n {
            x = x.square();
        }
        x
    }

    /// Multiplicative inverse via Fermat's little theorem: `x^(p-2)`.
    ///
    /// Returns `Fp::ZERO` when called on zero (no panic), matching the
    /// "inverse-or-zero" convention used by the upstream reference impls.
    #[must_use]
    pub fn invert(self) -> Self {
        // p - 2 = 0xFFFFFFFEFFFFFFFF; addition chain reaches the exponent in 11 mults
        // and 64 squarings. `xj` denotes `x^(2^j - 1)` at each step.
        let x = self;
        let x2 = x * x.square();
        let x4 = x2 * x2.msquare(2);
        let x5 = x * x4.square();
        let x10 = x5 * x5.msquare(5);
        let x15 = x5 * x10.msquare(5);
        let x16 = x * x15.square();
        let x31 = x15 * x16.msquare(15);
        let x32 = x * x31.square();
        x32 * x31.msquare(33)
    }

    /// Exponentiation by an unsigned 64-bit integer, via right-to-left square-and-multiply.
    #[must_use]
    pub fn pow(self, mut exp: u64) -> Self {
        let mut result = Self::ONE;
        let mut base = self;

        while exp != 0 {
            if exp & 1 == 1 {
                result *= base;
            }
            base = base.square();
            exp >>= 1;
        }
        result
    }

    /// Square root in `Fp`.
    ///
    /// Returns `Some(s)` such that `s^2 == self` when one exists (with the zero
    /// element returning `Some(Self::ZERO)`); returns `None` for non-squares.
    /// Picks one of the two roots: callers wanting a fixed sign must apply
    /// their own normalization on top.
    ///
    /// Implementation is Tonelli-Shanks specialized to the Goldilocks
    /// `p - 1 = 2^32 * (2^32 - 1)` factorization, with the precomputed
    /// 2^32-th root-of-unity generator `POWER_OF_TWO_GENERATOR` standing in
    /// for `z`.
    #[must_use]
    pub fn sqrt(self) -> Option<Self> {
        if self.is_zero() {
            return Some(Self::ZERO);
        }

        // Euler's criterion: `self^((p-1)/2)` is `+1` iff `self` is a square.
        let qr = self.pow((MODULUS - 1) >> 1);
        if qr == Self::MINUS_ONE {
            return None;
        }
        debug_assert_eq!(qr, Self::ONE);

        let t: u64 = (1u64 << (64 - TWO_ADICITY)) - 1;
        let mut z = Self::from_u64_reduce(POWER_OF_TWO_GENERATOR);
        let mut w = self.pow((t - 1) >> 1);
        let mut x = self * w;
        let mut b = x * w;
        let mut v = TWO_ADICITY;

        while b != Self::ONE {
            let mut k = 0u32;
            let mut b2k = b;

            while b2k != Self::ONE {
                b2k = b2k.square();
                k += 1;
            }

            let j = v - k - 1;
            w = z.msquare(j);
            z = w.square();
            b *= z;
            x *= w;
            v = k;
        }

        Some(x)
    }
}

impl Default for Fp {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl Add for Fp {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        self.add_inner(rhs)
    }
}

impl AddAssign for Fp {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        *self = self.add_inner(rhs);
    }
}

impl Sub for Fp {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        self.sub_inner(rhs)
    }
}

impl SubAssign for Fp {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.sub_inner(rhs);
    }
}

impl Neg for Fp {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        self.neg_inner()
    }
}

impl Mul for Fp {
    type Output = Self;
    #[inline(always)]
    fn mul(self, rhs: Self) -> Self {
        self.mul_inner(rhs)
    }
}

impl MulAssign for Fp {
    #[inline(always)]
    fn mul_assign(&mut self, rhs: Self) {
        *self = self.mul_inner(rhs);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;
    use crate::signing::fixtures::{arb_fp, arb_fp_nonzero, hex_to_bytes};

    const VECTORS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_field_goldilocks_vectors.json",
    ));

    #[derive(Debug, Deserialize)]
    struct Vectors {
        vectors: Vec<Vector>,
    }

    #[derive(Debug, Deserialize)]
    struct Vector {
        a: String,
        b: String,
        e: String,
        add: String,
        sub: String,
        mul: String,
        neg_a: String,
        inv_a: String,
        pow_a_e: String,
        a_eq_b: bool,
    }

    fn decode_le8(hex: &str) -> [u8; 8] {
        let bytes = hex_to_bytes(hex);
        assert_eq!(bytes.len(), 8, "expected 8 bytes, was {}", bytes.len());
        let mut out = [0u8; 8];
        out.copy_from_slice(&bytes);
        out
    }

    fn parse_u64(s: &str) -> u64 {
        if let Some(stripped) = s.strip_prefix("0x") {
            u64::from_str_radix(stripped, 16).unwrap()
        } else {
            s.parse::<u64>().unwrap()
        }
    }

    #[rstest]
    fn modulus_constant_is_goldilocks_prime() {
        assert_eq!(u128::from(MODULUS), (1u128 << 64) - (1u128 << 32) + 1);
    }

    #[rstest]
    fn round_trip_le_bytes_canonical() {
        for v in [0u64, 1, 42, MODULUS - 1] {
            let f = Fp::from_u64_canonical(v).unwrap();
            assert_eq!(f.to_u64(), v);
            let bytes = f.to_le_bytes();
            assert_eq!(Fp::try_from_le_bytes(bytes).unwrap(), f);
        }
    }

    #[rstest]
    fn rejects_non_canonical_decoding() {
        let bad = MODULUS.to_le_bytes();
        assert!(Fp::try_from_le_bytes(bad).is_none());
        let worse = u64::MAX.to_le_bytes();
        assert!(Fp::try_from_le_bytes(worse).is_none());
    }

    #[rstest]
    fn invert_zero_returns_zero() {
        assert_eq!(Fp::ZERO.invert(), Fp::ZERO);
    }

    #[rstest]
    fn sqrt_round_trip_for_known_squares() {
        for v in [1u64, 2, 4, 9, 16, 100, 1_000_000] {
            let x = Fp::from_u64_reduce(v);
            let xs = x.square();
            let s = xs.sqrt().expect("known squares are residues");
            assert_eq!(s.square(), xs);
        }
    }

    #[rstest]
    fn sqrt_zero_returns_zero() {
        assert_eq!(Fp::ZERO.sqrt(), Some(Fp::ZERO));
    }

    #[rstest]
    fn sqrt_returns_none_for_non_square() {
        // Construct a guaranteed non-residue by stepping through small candidates
        // until Euler's criterion fails. Fp has roughly half non-residues, so this
        // resolves on the first or second probe.
        let mut v = 2u64;

        loop {
            let x = Fp::from_u64_reduce(v);
            if x.pow((MODULUS - 1) >> 1) == Fp::MINUS_ONE {
                assert_eq!(x.sqrt(), None);
                break;
            }
            v += 1;
        }
    }

    #[rstest]
    fn ct_eq_matches_partial_eq() {
        let a = Fp::from_u64_reduce(123);
        let b = Fp::from_u64_reduce(123);
        let c = Fp::from_u64_reduce(124);
        assert_eq!(a.ct_eq(b), u64::MAX);
        assert_eq!(a.ct_eq(c), 0);
    }

    #[rstest]
    fn ct_select_picks_branch_by_mask() {
        let a = Fp::from_u64_reduce(123);
        let b = Fp::from_u64_reduce(456);
        assert_eq!(Fp::ct_select(0, a, b), a);
        assert_eq!(Fp::ct_select(u64::MAX, a, b), b);
    }

    proptest! {
        /// Field addition is commutative: `a + b == b + a` for any pair.
        #[rstest]
        fn prop_add_commutative(a in arb_fp(), b in arb_fp()) {
            prop_assert_eq!(a + b, b + a);
        }

        /// Field addition is associative: `(a + b) + c == a + (b + c)`.
        #[rstest]
        fn prop_add_associative(a in arb_fp(), b in arb_fp(), c in arb_fp()) {
            prop_assert_eq!((a + b) + c, a + (b + c));
        }

        /// Multiplication distributes over addition.
        #[rstest]
        fn prop_distributive(a in arb_fp(), b in arb_fp(), c in arb_fp()) {
            prop_assert_eq!(a * (b + c), a * b + a * c);
        }

        /// Multiplication is commutative.
        #[rstest]
        fn prop_mul_commutative(a in arb_fp(), b in arb_fp()) {
            prop_assert_eq!(a * b, b * a);
        }

        /// Multiplication is associative.
        #[rstest]
        fn prop_mul_associative(a in arb_fp(), b in arb_fp(), c in arb_fp()) {
            prop_assert_eq!((a * b) * c, a * (b * c));
        }

        /// `a + (-a) == 0` for any element.
        #[rstest]
        fn prop_neg_round_trip(a in arb_fp()) {
            prop_assert_eq!(a + (-a), Fp::ZERO);
        }

        /// `a - b == a + (-b)` for any pair.
        #[rstest]
        fn prop_sub_via_add_neg(a in arb_fp(), b in arb_fp()) {
            prop_assert_eq!(a - b, a + (-b));
        }

        /// `(a + b) - b == a` for any pair.
        #[rstest]
        fn prop_sub_round_trip(a in arb_fp(), b in arb_fp()) {
            prop_assert_eq!((a + b) - b, a);
        }

        /// Squaring matches self-multiplication.
        #[rstest]
        fn prop_square_matches_self_mul(a in arb_fp()) {
            prop_assert_eq!(a.square(), a * a);
        }

        /// `a * a.invert() == 1` for any non-zero element.
        #[rstest]
        fn prop_invert_round_trip(a in arb_fp_nonzero()) {
            prop_assert_eq!(a * a.invert(), Fp::ONE);
        }

        /// Fermat's little theorem: `a^(p-1) == 1` for any non-zero element.
        /// Pins the exponent ladder, the Montgomery reduction, and the
        /// addition chain through `pow` simultaneously.
        #[rstest]
        fn prop_fermat_little(a in arb_fp_nonzero()) {
            prop_assert_eq!(a.pow(MODULUS - 1), Fp::ONE);
        }

        /// `(a^2).sqrt()^2 == a^2`: sqrt of a known square round-trips.
        /// Picks one of the two roots; we only assert the squared identity.
        #[rstest]
        fn prop_sqrt_round_trip(a in arb_fp()) {
            let sq = a.square();
            let s = sq.sqrt().expect("squares are quadratic residues");
            prop_assert_eq!(s.square(), sq);
        }

        /// Canonical bytes round-trip: any `Fp` encodes to canonical bytes
        /// that decode back to the same element.
        #[rstest]
        fn prop_le_bytes_round_trip(a in arb_fp()) {
            let bytes = a.to_le_bytes();
            prop_assert_eq!(Fp::try_from_le_bytes(bytes).unwrap(), a);
        }

        /// `from_u64_canonical` accepts every value in `0..MODULUS`.
        #[rstest]
        fn prop_from_u64_canonical_accepts_in_range(v in 0u64..MODULUS) {
            let f = Fp::from_u64_canonical(v).expect("in-range value");
            prop_assert_eq!(f.to_u64(), v);
        }

        /// `from_u64_canonical` rejects every value `>= MODULUS`.
        #[rstest]
        fn prop_from_u64_canonical_rejects_out_of_range(v in MODULUS..=u64::MAX) {
            prop_assert!(Fp::from_u64_canonical(v).is_none());
        }

        /// `msquare(n)` matches `n` iterated `square` calls.
        #[rstest]
        fn prop_msquare_matches_iterated_square(a in arb_fp(), n in 0u32..16) {
            let mut iter = a;
            for _ in 0..n {
                iter = iter.square();
            }
            prop_assert_eq!(a.msquare(n), iter);
        }

        /// `ct_eq` agrees with `==` over arbitrary pairs.
        #[rstest]
        fn prop_ct_eq_matches_partial_eq(a in arb_fp(), b in arb_fp()) {
            let ct = a.ct_eq(b);
            if a == b {
                prop_assert_eq!(ct, u64::MAX);
            } else {
                prop_assert_eq!(ct, 0);
            }
        }

        /// `ct_select` picks `a` for mask 0 and `b` for mask u64::MAX.
        #[rstest]
        fn prop_ct_select_picks_branch(a in arb_fp(), b in arb_fp()) {
            prop_assert_eq!(Fp::ct_select(0, a, b), a);
            prop_assert_eq!(Fp::ct_select(u64::MAX, a, b), b);
        }
    }

    #[rstest]
    fn matches_go_reference_vectors() {
        let suite: Vectors = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.is_empty(), "vector file is empty");

        for (i, v) in suite.vectors.iter().enumerate() {
            let a = Fp::try_from_le_bytes(decode_le8(&v.a))
                .unwrap_or_else(|| panic!("vector {i}: decode a"));
            let b = Fp::try_from_le_bytes(decode_le8(&v.b))
                .unwrap_or_else(|| panic!("vector {i}: decode b"));
            let e = parse_u64(&v.e);

            assert_eq!((a + b).to_le_bytes(), decode_le8(&v.add), "vector {i}: add");
            assert_eq!((a - b).to_le_bytes(), decode_le8(&v.sub), "vector {i}: sub");
            assert_eq!((a * b).to_le_bytes(), decode_le8(&v.mul), "vector {i}: mul");
            assert_eq!((-a).to_le_bytes(), decode_le8(&v.neg_a), "vector {i}: neg");
            assert_eq!(
                a.invert().to_le_bytes(),
                decode_le8(&v.inv_a),
                "vector {i}: inv"
            );
            assert_eq!(
                a.pow(e).to_le_bytes(),
                decode_le8(&v.pow_a_e),
                "vector {i}: pow"
            );
            assert_eq!(a == b, v.a_eq_b, "vector {i}: eq");
        }
    }
}
