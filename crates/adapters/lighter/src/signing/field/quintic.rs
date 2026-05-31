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

//! Quintic extension `Fp5 = GF(p^5)` of the Goldilocks field.
//!
//! Elements are represented as `(c0, c1, c2, c3, c4)` over [`Fp`], encoding the
//! polynomial `c0 + c1*z + c2*z^2 + c3*z^3 + c4*z^4` modulo the irreducible
//! `z^5 - 3` (so `z^5 ≡ 3` in `Fp5`). Multiplication folds the schoolbook
//! cross-products with `W = 3` for the wraparound terms and applies Montgomery
//! reduction once per output coefficient. Inversion uses the Itoh-Tsujii trick
//! over the Frobenius `phi(x) = x^p`, reducing the work to three Frobenius
//! applications, two `Fp5` multiplications, and one `Fp` inversion.
//!
//! Arithmetic, inversion, and the [`Fp5::legendre`] descent inherit `Fp`'s
//! constant-time guarantees. [`Fp5::sqrt`] / [`Fp5::canonical_sqrt`] inherit
//! the variable-time behaviour of [`Fp::sqrt`] and are intended for the
//! public-input curve decode path; do not feed them secret operands.

use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use super::goldilocks::Fp;

/// Wraparound constant: `z^5 ≡ W (mod z^5 - W)` with `W = 3`.
const W: u64 = 3;

/// `d`-th root of unity used by the Frobenius operator, with `d = 5`.
///
/// For the irreducible `z^5 - W` and `p ≡ 1 (mod 5)`, the action `phi(z) = z^p`
/// reduces to `W^((p-1)/5) * z`, so this constant is `W^((p-1)/5) mod p`
/// (i.e. `3^((p-1)/5)` here, with `W = 3`). Precomputed as a Goldilocks element.
const DTH_ROOT: u64 = 1_041_288_259_238_279_555;

/// An element of `Fp5 = GF(p^5)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fp5(pub [Fp; 5]);

impl Fp5 {
    /// Additive identity.
    pub const ZERO: Self = Self([Fp::ZERO; 5]);

    /// Multiplicative identity.
    pub const ONE: Self = Self([Fp::ONE, Fp::ZERO, Fp::ZERO, Fp::ZERO, Fp::ZERO]);

    /// Build an element from five `u64` coefficients (low-to-high degree), each reduced mod `p`.
    #[inline]
    pub const fn from_u64s_reduce(c: [u64; 5]) -> Self {
        Self([
            Fp::from_u64_reduce(c[0]),
            Fp::from_u64_reduce(c[1]),
            Fp::from_u64_reduce(c[2]),
            Fp::from_u64_reduce(c[3]),
            Fp::from_u64_reduce(c[4]),
        ])
    }

    /// Build an element from five canonical `u64` coefficients; returns `None`
    /// if any coefficient is `>= p`.
    pub fn from_u64s_canonical(c: [u64; 5]) -> Option<Self> {
        Some(Self([
            Fp::from_u64_canonical(c[0])?,
            Fp::from_u64_canonical(c[1])?,
            Fp::from_u64_canonical(c[2])?,
            Fp::from_u64_canonical(c[3])?,
            Fp::from_u64_canonical(c[4])?,
        ]))
    }

    /// Decode an element from 40 little-endian bytes (5 x 8-byte canonical limbs).
    ///
    /// Returns `None` if any limb is non-canonical (`>= p`).
    pub fn try_from_le_bytes(bytes: [u8; 40]) -> Option<Self> {
        let mut limbs = [0u64; 5];
        for (i, limb) in limbs.iter_mut().enumerate() {
            let mut chunk = [0u8; 8];
            chunk.copy_from_slice(&bytes[i * 8..(i + 1) * 8]);
            *limb = u64::from_le_bytes(chunk);
        }
        Self::from_u64s_canonical(limbs)
    }

    /// Canonical 40-byte little-endian encoding (5 x 8-byte limbs, low-to-high degree).
    pub fn to_le_bytes(self) -> [u8; 40] {
        let mut out = [0u8; 40];
        for i in 0..5 {
            out[i * 8..(i + 1) * 8].copy_from_slice(&self.0[i].to_le_bytes());
        }
        out
    }

    /// Test whether the element is zero.
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0[0].is_zero()
            && self.0[1].is_zero()
            && self.0[2].is_zero()
            && self.0[3].is_zero()
            && self.0[4].is_zero()
    }

    /// Constant-time equality: returns `0xFFFF_FFFF_FFFF_FFFF` on equality, `0` otherwise.
    #[inline]
    pub fn ct_eq(self, rhs: Self) -> u64 {
        let z = (self.0[0].0 ^ rhs.0[0].0)
            | (self.0[1].0 ^ rhs.0[1].0)
            | (self.0[2].0 ^ rhs.0[2].0)
            | (self.0[3].0 ^ rhs.0[3].0)
            | (self.0[4].0 ^ rhs.0[4].0);
        ((z | z.wrapping_neg()) >> 63).wrapping_sub(1)
    }

    /// Branch-free select: returns `a` when `mask == 0` and `b` when
    /// `mask == u64::MAX`. Composed coefficient-wise from [`Fp::ct_select`];
    /// the secret-scalar curve primitives only ever pass full-bit masks.
    #[inline]
    #[must_use]
    pub fn ct_select(mask: u64, a: Self, b: Self) -> Self {
        Self([
            Fp::ct_select(mask, a.0[0], b.0[0]),
            Fp::ct_select(mask, a.0[1], b.0[1]),
            Fp::ct_select(mask, a.0[2], b.0[2]),
            Fp::ct_select(mask, a.0[3], b.0[3]),
            Fp::ct_select(mask, a.0[4], b.0[4]),
        ])
    }

    #[inline]
    fn add_inner(self, rhs: Self) -> Self {
        Self([
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
        ])
    }

    #[inline]
    fn sub_inner(self, rhs: Self) -> Self {
        Self([
            self.0[0] - rhs.0[0],
            self.0[1] - rhs.0[1],
            self.0[2] - rhs.0[2],
            self.0[3] - rhs.0[3],
            self.0[4] - rhs.0[4],
        ])
    }

    #[inline]
    fn neg_inner(self) -> Self {
        Self([-self.0[0], -self.0[1], -self.0[2], -self.0[3], -self.0[4]])
    }

    #[inline]
    fn scalar_mul(self, scalar: Fp) -> Self {
        Self([
            self.0[0] * scalar,
            self.0[1] * scalar,
            self.0[2] * scalar,
            self.0[3] * scalar,
            self.0[4] * scalar,
        ])
    }

    #[inline]
    fn mul_inner(self, rhs: Self) -> Self {
        let w = Fp::from_u64_reduce(W);
        let a = &self.0;
        let b = &rhs.0;

        // Schoolbook cross-product with `z^5 = W = 3`.
        let c0 = a[0] * b[0] + w * (a[1] * b[4] + a[2] * b[3] + a[3] * b[2] + a[4] * b[1]);
        let c1 = a[0] * b[1] + a[1] * b[0] + w * (a[2] * b[4] + a[3] * b[3] + a[4] * b[2]);
        let c2 = a[0] * b[2] + a[1] * b[1] + a[2] * b[0] + w * (a[3] * b[4] + a[4] * b[3]);
        let c3 = a[0] * b[3] + a[1] * b[2] + a[2] * b[1] + a[3] * b[0] + w * (a[4] * b[4]);
        let c4 = a[0] * b[4] + a[1] * b[3] + a[2] * b[2] + a[3] * b[1] + a[4] * b[0];

        Self([c0, c1, c2, c3, c4])
    }

    /// Squaring in `Fp5`.
    #[inline]
    #[must_use]
    pub fn square(self) -> Self {
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

    /// Frobenius operator: `phi(x) = x^p`.
    ///
    /// Acts on `(c0, c1, c2, c3, c4)` as multiplication of each higher-degree
    /// coefficient by a precomputed power of the `d`-th root of unity in `Fp`.
    #[inline]
    fn frobenius(self) -> Self {
        // Coefficients = `DTH_ROOT^i` for i=0..4 (i=0 fixed at 1).
        // DTH_ROOT^2 = 15820824984080659046, DTH_ROOT^3 = 211587555138949697,
        // DTH_ROOT^4 = 1373043270956696022 (matches Pornin and elliottech).
        Self([
            self.0[0],
            self.0[1] * Fp::from_u64_reduce(DTH_ROOT),
            self.0[2] * Fp::from_u64_reduce(15_820_824_984_080_659_046),
            self.0[3] * Fp::from_u64_reduce(211_587_555_138_949_697),
            self.0[4] * Fp::from_u64_reduce(1_373_043_270_956_696_022),
        ])
    }

    /// Frobenius applied twice: `x^(p^2)`.
    #[inline]
    fn frobenius2(self) -> Self {
        Self([
            self.0[0],
            self.0[1] * Fp::from_u64_reduce(15_820_824_984_080_659_046),
            self.0[2] * Fp::from_u64_reduce(1_373_043_270_956_696_022),
            self.0[3] * Fp::from_u64_reduce(DTH_ROOT),
            self.0[4] * Fp::from_u64_reduce(211_587_555_138_949_697),
        ])
    }

    /// Double in `Fp5`: returns `self + self`.
    #[inline]
    #[must_use]
    pub fn double(self) -> Self {
        self.add_inner(self)
    }

    /// Sign indicator following the elliottech Go reference convention. Used
    /// by [`Self::canonical_sqrt`] to fix the sign of square roots.
    ///
    /// The latch `sign = sign || (zero && sign_i)` with `sign_i = (limb is
    /// even)` reproduces the upstream behaviour bit-for-bit, including a
    /// known wrinkle: an element whose first non-zero coefficient is preceded
    /// by zero coefficients (e.g. `[0, 1, 0, 0, 0]`) reports `true` because a
    /// leading zero satisfies `sign_i`. This wrinkle has no observable effect
    /// on [`super::super::curve`]'s `Point::decode`: a flipped `r` swaps
    /// `x1`/`x2` contents, the subsequent Legendre check then re-selects the
    /// same non-square root, and the resulting `x` is identical. Phase E
    /// Layer 2 oracle tests against the Lighter Python SDK gate any
    /// divergence from the closed-source mainnet signer.
    #[must_use]
    pub fn sgn0(self) -> bool {
        let mut sign = false;
        let mut zero = true;

        for limb in &self.0 {
            let sign_i = (limb.to_u64() & 1) == 0;
            let zero_i = limb.is_zero();
            sign = sign || (zero && sign_i);
            zero = zero && zero_i;
        }
        sign
    }

    /// Legendre symbol of `self` in `Fp5`, returned as a base-field element.
    ///
    /// Returns `Fp::ZERO` for the zero element, `Fp::ONE` for non-zero squares,
    /// and `Fp::MINUS_ONE` for non-squares. Uses the Itoh-Tsujii descent into
    /// `Fp` followed by Euler's criterion split as `x^(2^63) / x^(2^31)`.
    #[must_use]
    pub fn legendre(self) -> Fp {
        let phi1 = self.frobenius();
        let phi1_phi2 = phi1 * phi1.frobenius();
        let xr_minus_1 = phi1_phi2 * phi1_phi2.frobenius2();

        let a = &self.0;
        let f = &xr_minus_1.0;
        let w = Fp::from_u64_reduce(W);
        let xr = a[0] * f[0] + w * (a[1] * f[4] + a[2] * f[3] + a[3] * f[2] + a[4] * f[1]);

        let xr31 = xr.msquare(31);
        let xr63 = xr31.msquare(32);
        xr63 * xr31.invert()
    }

    /// Square root in `Fp5` via descent to `Fp`.
    ///
    /// Returns `Some(s)` such that `s^2 == self` when one exists (`Some(ZERO)`
    /// for the zero input); returns `None` for non-squares. The chosen root is
    /// arbitrary within the two square roots; use [`Self::canonical_sqrt`] for
    /// a deterministic sign.
    #[must_use]
    pub fn sqrt(self) -> Option<Self> {
        // Repeated squaring lifts `self` to `Fp`-valued exponents; specifically
        // `g = self^(1 + p + p^2 + p^3 + p^4)` lives in `Fp`. We compute an
        // intermediate `e` such that `e^2 * g == self^N` for an odd `N`, take
        // the square root in `Fp`, and divide back through.
        let v = self.msquare(31);
        let d = self * v.msquare(32) * v.invert();
        let e = (d * d.frobenius2()).frobenius();
        let f_sq = e.square();

        let a = &self.0;
        let f = &f_sq.0;
        let w = Fp::from_u64_reduce(W);
        let g = a[0] * f[0] + w * (a[1] * f[4] + a[2] * f[3] + a[3] * f[2] + a[4] * f[1]);

        let s = g.sqrt()?;
        let e_inv = e.invert();
        Some(Self([s, Fp::ZERO, Fp::ZERO, Fp::ZERO, Fp::ZERO]) * e_inv)
    }

    /// Canonical-sign square root: same as [`Self::sqrt`], with the result
    /// negated whenever its first non-zero coefficient is even (per [`Self::sgn0`]).
    #[must_use]
    pub fn canonical_sqrt(self) -> Option<Self> {
        let s = self.sqrt()?;
        if s.sgn0() { Some(-s) } else { Some(s) }
    }

    /// Multiplicative inverse via Itoh-Tsujii. Returns `Fp5::ZERO` on input zero.
    ///
    /// With `r = 1 + p + p^2 + p^3 + p^4`, the value `x^r` lands in the base
    /// field `Fp`, so we compute `x^(r-1)` cheaply via Frobenius, recover
    /// `x^r = x_0 * x^(r-1)|_0` inside `Fp`, and divide. The branch-free
    /// shape preserves the module's constant-time contract: a zero input
    /// flows through the Frobenius cascade as zero and `Fp::invert(0) = 0`
    /// folds back into a zero result without an early return.
    #[must_use]
    pub fn invert(self) -> Self {
        let phi1 = self.frobenius();
        let phi1_phi2 = phi1 * phi1.frobenius();
        let xr_minus_1 = phi1_phi2 * phi1_phi2.frobenius2();

        // `xr` lives in `Fp` (degree-zero coefficient of `self * xr_minus_1`).
        let a = &self.0;
        let f = &xr_minus_1.0;
        let w = Fp::from_u64_reduce(W);
        let xr = a[0] * f[0] + w * (a[1] * f[4] + a[2] * f[3] + a[3] * f[2] + a[4] * f[1]);

        xr_minus_1.scalar_mul(xr.invert())
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
}

impl Default for Fp5 {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl Add for Fp5 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.add_inner(rhs)
    }
}

impl AddAssign for Fp5 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = self.add_inner(rhs);
    }
}

impl Sub for Fp5 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self.sub_inner(rhs)
    }
}

impl SubAssign for Fp5 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.sub_inner(rhs);
    }
}

impl Neg for Fp5 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        self.neg_inner()
    }
}

impl Mul for Fp5 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        self.mul_inner(rhs)
    }
}

impl MulAssign for Fp5 {
    #[inline]
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
    use crate::signing::{
        field::MODULUS,
        fixtures::{arb_fp5, arb_fp5_nonzero, hex_to_bytes},
    };

    const VECTORS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_field_quintic_vectors.json",
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

    fn decode_le40(hex: &str) -> [u8; 40] {
        let bytes = hex_to_bytes(hex);
        assert_eq!(bytes.len(), 40, "expected 40 bytes, was {}", bytes.len());
        let mut out = [0u8; 40];
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
    fn round_trip_le_bytes_canonical() {
        let v = Fp5::from_u64s_reduce([1, 2, 3, 4, 5]);
        let bytes = v.to_le_bytes();
        assert_eq!(Fp5::try_from_le_bytes(bytes).unwrap(), v);
    }

    #[rstest]
    fn one_is_multiplicative_identity() {
        let v = Fp5::from_u64s_reduce([7, 11, 13, 17, 19]);
        assert_eq!(v * Fp5::ONE, v);
        assert_eq!(Fp5::ONE * v, v);
    }

    #[rstest]
    fn invert_zero_returns_zero() {
        assert_eq!(Fp5::ZERO.invert(), Fp5::ZERO);
    }

    #[rstest]
    fn invert_round_trip() {
        let v = Fp5::from_u64s_reduce([7, 11, 13, 17, 19]);
        assert_eq!(v * v.invert(), Fp5::ONE);
    }

    #[rstest]
    fn double_matches_self_addition() {
        let v = Fp5::from_u64s_reduce([1, 2, 3, 4, 5]);
        assert_eq!(v.double(), v + v);
    }

    #[rstest]
    fn ct_select_picks_branch_by_mask() {
        let a = Fp5::from_u64s_reduce([1, 2, 3, 4, 5]);
        let b = Fp5::from_u64s_reduce([10, 20, 30, 40, 50]);
        assert_eq!(Fp5::ct_select(0, a, b), a);
        assert_eq!(Fp5::ct_select(u64::MAX, a, b), b);
    }

    #[rstest]
    fn legendre_classifies_squares() {
        let v = Fp5::from_u64s_reduce([7, 11, 13, 17, 19]);
        let v_sq = v.square();
        assert_eq!(v_sq.legendre(), Fp::ONE);
        assert_eq!(Fp5::ZERO.legendre(), Fp::ZERO);
    }

    #[rstest]
    fn sqrt_round_trip_for_squares() {
        let v = Fp5::from_u64s_reduce([7, 11, 13, 17, 19]);
        let v_sq = v.square();
        let s = v_sq.sqrt().expect("v_sq is a square by construction");
        assert_eq!(s.square(), v_sq);
    }

    #[rstest]
    fn canonical_sqrt_picks_odd_first_limb() {
        let v = Fp5::from_u64s_reduce([7, 11, 13, 17, 19]);
        let v_sq = v.square();
        let s = v_sq
            .canonical_sqrt()
            .expect("v_sq is a square by construction");
        assert_eq!(s.square(), v_sq);
        assert!(!s.sgn0(), "canonical_sqrt result must have sgn0 == false");
    }

    /// `from_u64s_canonical` rejects any non-canonical limb (`>= MODULUS`),
    /// for each of the five limb positions in turn.
    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(2)]
    #[case(3)]
    #[case(4)]
    fn from_u64s_canonical_rejects_non_canonical_limb(#[case] limb_index: usize) {
        let mut limbs = [1u64, 2, 3, 4, 5];
        limbs[limb_index] = MODULUS;
        assert!(
            Fp5::from_u64s_canonical(limbs).is_none(),
            "limb {limb_index} == MODULUS must be rejected",
        );

        limbs[limb_index] = u64::MAX;
        assert!(
            Fp5::from_u64s_canonical(limbs).is_none(),
            "limb {limb_index} == u64::MAX must be rejected",
        );
    }

    proptest! {
        /// `Fp5` addition is commutative.
        #[rstest]
        fn prop_add_commutative(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!(a + b, b + a);
        }

        /// `Fp5` addition is associative.
        #[rstest]
        fn prop_add_associative(a in arb_fp5(), b in arb_fp5(), c in arb_fp5()) {
            prop_assert_eq!((a + b) + c, a + (b + c));
        }

        /// Multiplication distributes over addition.
        #[rstest]
        fn prop_distributive(a in arb_fp5(), b in arb_fp5(), c in arb_fp5()) {
            prop_assert_eq!(a * (b + c), a * b + a * c);
        }

        /// Multiplication is commutative.
        #[rstest]
        fn prop_mul_commutative(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!(a * b, b * a);
        }

        /// Multiplication is associative.
        #[rstest]
        fn prop_mul_associative(a in arb_fp5(), b in arb_fp5(), c in arb_fp5()) {
            prop_assert_eq!((a * b) * c, a * (b * c));
        }

        /// `a + (-a) == 0`.
        #[rstest]
        fn prop_neg_round_trip(a in arb_fp5()) {
            prop_assert_eq!(a + (-a), Fp5::ZERO);
        }

        /// `a - b == a + (-b)`.
        #[rstest]
        fn prop_sub_via_add_neg(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!(a - b, a + (-b));
        }

        /// `(a + b) - b == a`.
        #[rstest]
        fn prop_sub_round_trip(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!((a + b) - b, a);
        }

        /// Squaring matches self-multiplication.
        #[rstest]
        fn prop_square_matches_self_mul(a in arb_fp5()) {
            prop_assert_eq!(a.square(), a * a);
        }

        /// `double` matches self addition.
        #[rstest]
        fn prop_double_matches_self_addition(a in arb_fp5()) {
            prop_assert_eq!(a.double(), a + a);
        }

        /// `a * a.invert() == 1` for any non-zero element.
        #[rstest]
        fn prop_invert_round_trip(a in arb_fp5_nonzero()) {
            prop_assert_eq!(a * a.invert(), Fp5::ONE);
        }

        /// `(a^2).sqrt()^2 == a^2`: sqrt of any known square round-trips.
        #[rstest]
        fn prop_sqrt_round_trip(a in arb_fp5()) {
            let sq = a.square();
            let s = sq.sqrt().expect("squares are quadratic residues");
            prop_assert_eq!(s.square(), sq);
        }

        /// `canonical_sqrt(a^2)^2 == a^2`: the canonicalised root squares
        /// back to the input. The result's `sgn0` is NOT asserted here:
        /// when the root falls into the documented leading-zero wrinkle on
        /// `sgn0` (both root and its negation report `true`), `canonical_sqrt`
        /// returns the negated root which still reports `true`. That branch
        /// has no observable effect on `Point::decode` per the doc on
        /// `Fp5::sgn0`, and the byte-equality oracle vectors pin the wider
        /// behaviour end-to-end.
        #[rstest]
        fn prop_canonical_sqrt_round_trip(a in arb_fp5_nonzero()) {
            let sq = a.square();
            let s = sq.canonical_sqrt().expect("squares are quadratic residues");
            prop_assert_eq!(s.square(), sq);
        }

        /// `canonical_sqrt` is deterministic: invoking it twice on the same
        /// input produces the same root.
        #[rstest]
        fn prop_canonical_sqrt_deterministic(a in arb_fp5_nonzero()) {
            let sq = a.square();
            prop_assert_eq!(sq.canonical_sqrt(), sq.canonical_sqrt());
        }

        /// The Lighter-style sign latch is anti-symmetric for any element
        /// whose first coefficient is non-zero: exactly one of `x` and
        /// `-x` reports `sgn0 == true`. Pins the latch contract on the
        /// no-leading-zero branch documented at `Fp5::sgn0`. (The wrinkle
        /// where `c[0] == 0` makes both `x` and `-x` report `true` is
        /// excluded from this strategy by construction; the doc on
        /// `Fp5::sgn0` notes the wrinkle has no observable effect on the
        /// curve `decode` path that consumes this primitive.)
        #[rstest]
        fn prop_sgn0_negation_anti_symmetric(
            a in arb_fp5_nonzero().prop_filter("c0 nonzero", |x| !x.0[0].is_zero()),
        ) {
            prop_assert_ne!(a.sgn0(), (-a).sgn0());
        }

        /// `Fp5` Legendre symbol is multiplicative: `legendre(a*b) ==
        /// legendre(a) * legendre(b)` for non-zero operands.
        #[rstest]
        fn prop_legendre_multiplicative(a in arb_fp5_nonzero(), b in arb_fp5_nonzero()) {
            let prod = a * b;
            prop_assume!(!prod.is_zero());
            prop_assert_eq!(prod.legendre(), a.legendre() * b.legendre());
        }

        /// Squares produce Legendre `+1`.
        #[rstest]
        fn prop_legendre_square_is_one(a in arb_fp5_nonzero()) {
            prop_assert_eq!(a.square().legendre(), Fp::ONE);
        }

        /// `frobenius` applied five times is the identity (since `phi(x) = x^p`
        /// and `Fp5` has order `p^5 - 1`, `phi^5 = id`).
        #[rstest]
        fn prop_frobenius_iter_five_is_identity(a in arb_fp5()) {
            let phi5 = a.frobenius().frobenius().frobenius().frobenius().frobenius();
            prop_assert_eq!(phi5, a);
        }

        /// Frobenius is a ring homomorphism over multiplication.
        #[rstest]
        fn prop_frobenius_multiplicative(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!((a * b).frobenius(), a.frobenius() * b.frobenius());
        }

        /// `frobenius2` matches `frobenius` applied twice.
        #[rstest]
        fn prop_frobenius2_matches_double_frobenius(a in arb_fp5()) {
            prop_assert_eq!(a.frobenius2(), a.frobenius().frobenius());
        }

        /// Canonical bytes round-trip.
        #[rstest]
        fn prop_le_bytes_round_trip(a in arb_fp5()) {
            let bytes = a.to_le_bytes();
            prop_assert_eq!(Fp5::try_from_le_bytes(bytes).unwrap(), a);
        }

        /// `ct_select` picks `a` for mask 0 and `b` for mask u64::MAX.
        #[rstest]
        fn prop_ct_select_picks_branch(a in arb_fp5(), b in arb_fp5()) {
            prop_assert_eq!(Fp5::ct_select(0, a, b), a);
            prop_assert_eq!(Fp5::ct_select(u64::MAX, a, b), b);
        }

        /// `ct_eq` agrees with `==`.
        #[rstest]
        fn prop_ct_eq_matches_partial_eq(a in arb_fp5(), b in arb_fp5()) {
            let ct = a.ct_eq(b);
            if a == b {
                prop_assert_eq!(ct, u64::MAX);
            } else {
                prop_assert_eq!(ct, 0);
            }
        }
    }

    #[rstest]
    fn matches_go_reference_vectors() {
        let suite: Vectors = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.is_empty(), "vector file is empty");

        for (i, v) in suite.vectors.iter().enumerate() {
            let a = Fp5::try_from_le_bytes(decode_le40(&v.a))
                .unwrap_or_else(|| panic!("vector {i}: decode a"));
            let b = Fp5::try_from_le_bytes(decode_le40(&v.b))
                .unwrap_or_else(|| panic!("vector {i}: decode b"));
            let e = parse_u64(&v.e);

            assert_eq!(
                (a + b).to_le_bytes(),
                decode_le40(&v.add),
                "vector {i}: add"
            );
            assert_eq!(
                (a - b).to_le_bytes(),
                decode_le40(&v.sub),
                "vector {i}: sub"
            );
            assert_eq!(
                (a * b).to_le_bytes(),
                decode_le40(&v.mul),
                "vector {i}: mul"
            );
            assert_eq!((-a).to_le_bytes(), decode_le40(&v.neg_a), "vector {i}: neg");
            assert_eq!(
                a.invert().to_le_bytes(),
                decode_le40(&v.inv_a),
                "vector {i}: inv"
            );
            assert_eq!(
                a.pow(e).to_le_bytes(),
                decode_le40(&v.pow_a_e),
                "vector {i}: pow"
            );
            assert_eq!(a == b, v.a_eq_b, "vector {i}: eq");
        }
    }
}
