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

//! Scalar field of the ECgFp5 curve.
//!
//! Operates modulo the prime group order `n`, a 319-bit prime stored as five
//! 64-bit little-endian limbs in non-Montgomery form. Encoding/decoding and
//! addition/subtraction work directly on the limbs; multiplication uses
//! Montgomery form internally with the precomputed constants `R^2 mod n` and
//! `-1/n[0] mod 2^64`.
//!
//! All arithmetic primitives execute as branch-free limb-wise sequences so
//! timing reveals nothing about secret operands. The variable-time helpers are
//! confined to encode/decode-style boundaries (`from_le_bytes_reduce`) and the
//! signed-window recoding used by the variable-time scalar multiplication.

use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::signing::field::Fp5;

/// Number of 64-bit limbs in a scalar.
pub const LIMBS: usize = 5;

/// Canonical 40-byte little-endian encoding length for a scalar.
pub const SCALAR_BYTES: usize = LIMBS * 8;

/// Group order `n` of the ECgFp5 curve, a 319-bit prime.
///
/// `n = 1067993516717146951041484916571792702745057740581727230159139685185762082554198619328292418486241`
pub const ORDER: Scalar = Scalar([
    0xE80F_D996_948B_FFE1,
    0xE888_5C39_D724_A09C,
    0x7FFF_FFE6_CFB8_0639,
    0x7FFF_FFF1_0000_0016,
    0x7FFF_FFFD_8000_0007,
]);

/// `-1 / n[0] mod 2^64`, the precomputed Montgomery reduction multiplier.
const N0I: u64 = 0xD78B_EF72_057B_7BDF;

/// `R^2 mod n` with `R = 2^320`. Multiplying by this lifts a value into
/// Montgomery form via [`Scalar::monty_mul`].
const R2: Scalar = Scalar([
    0xA010_01DC_E33D_C739,
    0x6C32_28D3_3F62_ACCF,
    0xD1D7_96CC_91CF_8525,
    0xAADF_FF5D_1574_C1D8,
    0x4ACA_13B2_8CA2_51F5,
]);

/// A scalar in the ECgFp5 curve's scalar field, stored in non-Montgomery form
/// as five 64-bit little-endian limbs.
///
/// Canonical instances (`< n`) round-trip through every public operation. The
/// arithmetic primitives `add` / `sub` / `mul` require canonical inputs;
/// `from_le_bytes_reduce` handles the wider input range produced by hashing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scalar(pub [u64; LIMBS]);

impl Scalar {
    /// Additive identity.
    pub const ZERO: Self = Self([0; LIMBS]);

    /// Multiplicative identity.
    pub const ONE: Self = Self([1, 0, 0, 0, 0]);

    /// Build a scalar from five raw 64-bit limbs without any reduction.
    ///
    /// The caller is responsible for ensuring the value is canonical when it
    /// will subsequently feed the modular `Add`, `Sub`, `Neg` or `Mul`
    /// operators.
    #[inline]
    #[must_use]
    pub const fn from_limbs(limbs: [u64; LIMBS]) -> Self {
        Self(limbs)
    }

    /// Return the underlying limbs in little-endian order.
    #[inline]
    #[must_use]
    pub const fn to_limbs(self) -> [u64; LIMBS] {
        self.0
    }

    /// Test whether this scalar is canonical (`self < n`).
    #[must_use]
    pub fn is_canonical(self) -> bool {
        for i in (0..LIMBS).rev() {
            if self.0[i] < ORDER.0[i] {
                return true;
            }

            if self.0[i] > ORDER.0[i] {
                return false;
            }
        }
        false
    }

    /// Test whether the scalar is zero.
    #[inline]
    #[must_use]
    pub const fn is_zero(self) -> bool {
        let mut acc: u64 = 0;
        let mut i = 0;

        while i < LIMBS {
            acc |= self.0[i];
            i += 1;
        }
        acc == 0
    }

    /// Canonical 40-byte little-endian encoding (5 x 8-byte limbs).
    #[must_use]
    pub fn to_le_bytes(self) -> [u8; SCALAR_BYTES] {
        let mut out = [0u8; SCALAR_BYTES];

        for (i, limb) in self.0.iter().enumerate() {
            out[i * 8..(i + 1) * 8].copy_from_slice(&limb.to_le_bytes());
        }
        out
    }

    /// Decode 40 little-endian bytes, reducing modulo `n` if the encoded value
    /// exceeds the canonical range. Mirrors the upstream Lighter helper.
    #[must_use]
    pub fn from_le_bytes_reduce(bytes: [u8; SCALAR_BYTES]) -> Self {
        let mut limbs = [0u64; LIMBS];

        for (i, slot) in limbs.iter_mut().enumerate() {
            let mut chunk = [0u8; 8];
            chunk.copy_from_slice(&bytes[i * 8..(i + 1) * 8]);
            *slot = u64::from_le_bytes(chunk);
        }

        let s = Self(limbs);
        if s.is_canonical() {
            s
        } else {
            // The encoded integer is at most `2^320 - 1 < 4 * n`, so at most
            // two conditional subtractions land us in canonical range.
            let (s1, b1) = s.sub_inner(ORDER);
            let s_after1 = if b1 != 0 { s } else { s1 };
            let (s2, b2) = s_after1.sub_inner(ORDER);
            if b2 != 0 { s_after1 } else { s2 }
        }
    }

    /// Branch-free constant-time select: returns `a0` when `c == 0` and `a1`
    /// when `c == u64::MAX`.
    #[inline]
    #[must_use]
    pub const fn select(c: u64, a0: Self, a1: Self) -> Self {
        Self([
            a0.0[0] ^ (c & (a0.0[0] ^ a1.0[0])),
            a0.0[1] ^ (c & (a0.0[1] ^ a1.0[1])),
            a0.0[2] ^ (c & (a0.0[2] ^ a1.0[2])),
            a0.0[3] ^ (c & (a0.0[3] ^ a1.0[3])),
            a0.0[4] ^ (c & (a0.0[4] ^ a1.0[4])),
        ])
    }

    /// Raw 320-bit addition with no modular reduction.
    #[inline]
    #[must_use]
    pub fn add_inner(self, rhs: Self) -> Self {
        let mut r = [0u64; LIMBS];
        let mut carry: u8 = 0;

        for (i, slot) in r.iter_mut().enumerate() {
            let (t1, c1) = self.0[i].overflowing_add(rhs.0[i]);
            let (t2, c2) = t1.overflowing_add(u64::from(carry));
            *slot = t2;
            carry = u8::from(c1) | u8::from(c2);
        }
        Self(r)
    }

    /// Raw 320-bit subtraction with no modular reduction. Returns the
    /// difference and an `0` / `u64::MAX` mask indicating whether the operation
    /// borrowed beyond the top limb.
    #[inline]
    #[must_use]
    pub fn sub_inner(self, rhs: Self) -> (Self, u64) {
        let mut r = [0u64; LIMBS];
        let mut borrow: u8 = 0;

        for (i, slot) in r.iter_mut().enumerate() {
            let (t1, b1) = self.0[i].overflowing_sub(rhs.0[i]);
            let (t2, b2) = t1.overflowing_sub(u64::from(borrow));
            *slot = t2;
            borrow = u8::from(b1) | u8::from(b2);
        }
        let mask = if borrow != 0 { u64::MAX } else { 0 };
        (Self(r), mask)
    }

    /// Montgomery multiplication `(self * rhs) / 2^320 mod n`.
    ///
    /// `self` MUST be canonical. `rhs` may exceed `n` provided it fits in 320
    /// bits, mirroring the upstream behaviour used to lift values into
    /// Montgomery form via the `R2` constant.
    #[must_use]
    pub fn monty_mul(self, rhs: Self) -> Self {
        debug_assert!(self.is_canonical(), "Scalar::monty_mul: lhs not canonical",);

        let mut r = [0u64; LIMBS];

        for i in 0..LIMBS {
            let m = rhs.0[i];
            let f = (self.0[0].wrapping_mul(m).wrapping_add(r[0])).wrapping_mul(N0I);

            let mut cc1: u64 = 0;
            let mut cc2: u64 = 0;

            for j in 0..LIMBS {
                let prod = u128::from(self.0[j]) * u128::from(m);
                let s1 = prod + u128::from(r[j]) + u128::from(cc1);
                cc1 = (s1 >> 64) as u64;
                let s1_lo = s1 as u64;

                let prod_n = u128::from(f) * u128::from(ORDER.0[j]);
                let s2 = prod_n + u128::from(s1_lo) + u128::from(cc2);
                cc2 = (s2 >> 64) as u64;
                let s2_lo = s2 as u64;

                if j > 0 {
                    r[j - 1] = s2_lo;
                }
            }
            r[LIMBS - 1] = cc1.wrapping_add(cc2);
        }

        let r0 = Self(r);
        let (r1, c) = r0.sub_inner(ORDER);
        Self::select(c, r1, r0)
    }

    /// Build a scalar from an `Fp5` element via reduction modulo `n`.
    ///
    /// Concatenates the five canonical 64-bit limbs of `e` into a 320-bit
    /// little-endian integer and reduces, matching the upstream behaviour of
    /// `FromGfp5`. Used by the Schnorr binding to derive a scalar from a
    /// Poseidon2 digest.
    #[must_use]
    pub fn from_fp5(e: Fp5) -> Self {
        let mut bytes = [0u8; SCALAR_BYTES];
        let encoded = e.to_le_bytes();
        bytes.copy_from_slice(&encoded);
        Self::from_le_bytes_reduce(bytes)
    }

    /// Split the canonical scalar value into 80 little-endian 4-bit nibbles.
    ///
    /// Iteration order is least-significant nibble first.
    #[must_use]
    pub fn split_to_4_bit_limbs(self) -> [u8; 80] {
        let mut out = [0u8; 80];

        for i in 0..LIMBS {
            for j in 0..16 {
                out[i * 16 + j] = ((self.0[i] >> (j * 4)) & 0xF) as u8;
            }
        }
        out
    }

    /// Recode the scalar into signed digits for a width-`w` window.
    ///
    /// `ss` is filled with `(2^w + 1)`-range signed values (lying in
    /// `-(2^(w-1)) ..= 2^(w-1)` after carry propagation) so that
    /// `sum(ss[i] * 2^(w*i)) == self mod 2^(w * len)`. When `w * len >= 320`,
    /// the recoding spans the entire scalar and the top digit is non-negative.
    ///
    /// `w` MUST satisfy `2 <= w <= 10`. This helper is variable-time and is
    /// only suitable for non-secret window selection.
    pub fn recode_signed(self, ss: &mut [i32], w: u32) {
        recode_signed_from_limbs(&self.0, ss, w);
    }
}

/// Recode an arbitrary little-endian limb sequence into signed window digits.
/// Standalone helper exposed mainly for testing parity with the upstream code.
pub fn recode_signed_from_limbs(limbs: &[u64], ss: &mut [i32], w: u32) {
    debug_assert!((2..=10).contains(&w), "window width must be in 2..=10");

    let mw = (1u32 << w) - 1;
    let hw = 1u32 << (w - 1);

    let mut acc: u64 = 0;
    let mut acc_len: i32 = 0;
    let mut j: usize = 0;
    let mut cc: u32 = 0;
    let w_i32 = w as i32;

    for slot in ss.iter_mut() {
        let bb: u32 = if acc_len < w_i32 {
            if j < limbs.len() {
                let nl = limbs[j];
                j += 1;
                let bits = ((acc | (nl << acc_len)) as u32) & mw;
                acc = nl >> (w_i32 - acc_len);
                acc_len += 64 - w_i32;
                bits
            } else {
                let bits = (acc as u32) & mw;
                acc = 0;
                acc_len += 64 - w_i32;
                bits
            }
        } else {
            let bits = (acc as u32) & mw;
            acc_len -= w_i32;
            acc >>= w;
            bits
        };

        let sum = bb.wrapping_add(cc);
        cc = (hw.wrapping_sub(sum)) >> 31;
        *slot = (sum as i32) - ((cc << w) as i32);
    }
}

impl Default for Scalar {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl Add for Scalar {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        debug_assert!(self.is_canonical(), "Scalar add: lhs not canonical");
        debug_assert!(rhs.is_canonical(), "Scalar add: rhs not canonical");

        let r0 = self.add_inner(rhs);
        let (r1, c) = r0.sub_inner(ORDER);
        Self::select(c, r1, r0)
    }
}

impl AddAssign for Scalar {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Scalar {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        debug_assert!(self.is_canonical(), "Scalar sub: lhs not canonical");
        debug_assert!(rhs.is_canonical(), "Scalar sub: rhs not canonical");

        let (r0, c) = self.sub_inner(rhs);
        let r1 = r0.add_inner(ORDER);
        Self::select(c, r0, r1)
    }
}

impl SubAssign for Scalar {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Neg for Scalar {
    type Output = Self;

    fn neg(self) -> Self {
        debug_assert!(self.is_canonical(), "Scalar neg: input not canonical");
        Self::ZERO - self
    }
}

impl Mul for Scalar {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        debug_assert!(self.is_canonical(), "Scalar mul: lhs not canonical");
        debug_assert!(rhs.is_canonical(), "Scalar mul: rhs not canonical");

        self.monty_mul(R2).monty_mul(rhs)
    }
}

impl MulAssign for Scalar {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::signing::fixtures::{arb_scalar, arb_scalar_nonzero};

    #[rstest]
    fn order_round_trips_through_le_bytes() {
        let bytes = ORDER.to_le_bytes();
        // ORDER itself is not canonical (`< n` is false at equality), but the
        // round-trip-with-reduce should normalize it to zero.
        let s = Scalar::from_le_bytes_reduce(bytes);
        assert_eq!(s, Scalar::ZERO);
    }

    #[rstest]
    fn add_inner_carries_through_top_limb() {
        let scalar1 = Scalar([
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
        ]);
        let scalar2 = Scalar([
            0x00FF_FFFF_FFFE_EFFF,
            12_312_321_312,
            0xFFFF_FFFF_FFFF_FFFF,
            0x00FF_FFFF_FACD_FFFF,
            0xBCAF_FFFF_FFFF_FFFF,
        ]);
        let expected = Scalar([
            0x00FF_FFFF_FFFE_EFFE,
            0x0000_0002_DDDF_1D20,
            0xFFFF_FFFF_FFFF_FFFF,
            0x00FF_FFFF_FACD_FFFF,
            0xBCAF_FFFF_FFFF_FFFF,
        ]);
        assert_eq!(scalar1.add_inner(scalar2), expected);
    }

    #[rstest]
    fn sub_inner_signals_borrow() {
        let scalar1 = Scalar::ZERO;
        let scalar2 = Scalar([u64::MAX; 5]);
        let (result, borrow) = scalar1.sub_inner(scalar2);
        assert_eq!(result, Scalar([1, 0, 0, 0, 0]));
        assert_eq!(borrow, u64::MAX);
    }

    #[rstest]
    fn modular_sub_wraps_through_order() {
        let scalar1 = Scalar([1, 2, 0, 0, 0]);
        let scalar2 = Scalar([
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFF,
            0x0FFF_FFFF_FFFF_FFFF,
        ]);
        assert!(scalar2.is_canonical());

        let result = scalar1 - scalar2;
        let expected = Scalar([
            0xE80F_D996_948B_FFE3,
            0xE888_5C39_D724_A09E,
            0x7FFF_FFE6_CFB8_0639,
            0x7FFF_FFF1_0000_0016,
            8_070_450_521_510_510_599,
        ]);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn select_picks_branch_by_mask() {
        let a0 = Scalar([1, 2, 3, 4, 5]);
        let a1 = Scalar([
            0xFFFF_FFFF_FFFF_FFFF,
            0xFFFF_FFFF_FFFF_FFFE,
            0xFFFF_FFFF_FFFF_FFFD,
            0xFFFF_FFFF_FFFF_FFFC,
            0xFFFF_FFFF_FFFF_FFFB,
        ]);
        assert_eq!(Scalar::select(0, a0, a1), a0);
        assert_eq!(Scalar::select(u64::MAX, a0, a1), a1);
    }

    #[rstest]
    fn one_is_multiplicative_identity() {
        let s = Scalar([
            0x1234_5678_90AB_CDEF,
            0xFEDC_BA98_7654_3210,
            0x0123_4567_89AB_CDEF,
            0xFEDC_BA98_7654_3210,
            0x1234_5678_90AB_CDEF,
        ]);
        assert!(s.is_canonical());
        assert_eq!(s * Scalar::ONE, s);
        assert_eq!(Scalar::ONE * s, s);
    }

    #[rstest]
    fn neg_is_additive_inverse() {
        let s = Scalar([7, 11, 13, 17, 19]);
        let zero = s + (-s);
        assert_eq!(zero, Scalar::ZERO);
    }

    #[rstest]
    fn split_to_4_bit_limbs_matches_reference_vector() {
        let scalar = Scalar([
            6_950_590_877_883_398_434,
            17_178_336_263_794_770_543,
            11_012_823_478_139_181_320,
            16_445_091_359_523_510_936,
            5_882_925_226_143_600_273,
        ]);

        let limbs = scalar.split_to_4_bit_limbs();
        // Spot-check a handful of nibbles against the upstream Go vector.
        assert_eq!(limbs[0], 2);
        assert_eq!(limbs[1], 2);
        assert_eq!(limbs[2], 9);
        assert_eq!(limbs[16], 15);
        assert_eq!(limbs[39], 13);
        assert_eq!(limbs[79], 5);

        // Stronger: the nibble sequence reconstructs the original limbs.
        assert_eq!(reconstruct_from_4_bit_nibbles(limbs), scalar.0);
    }

    /// Reconstruct a 5-limb scalar by repacking 80 little-endian 4-bit nibbles
    /// back into 5 u64s.
    fn reconstruct_from_4_bit_nibbles(nibbles: [u8; 80]) -> [u64; LIMBS] {
        let mut out = [0u64; LIMBS];
        for (i, slot) in out.iter_mut().enumerate() {
            let mut v: u64 = 0;
            for j in 0..16 {
                v |= u64::from(nibbles[i * 16 + j]) << (j * 4);
            }
            *slot = v;
        }
        out
    }

    /// Reconstruct the 5-limb scalar value from its signed-window digits via
    /// `sum(ss[i] * 2^(w*i)) mod 2^320`. Used in tests to pin the recoding
    /// spec.
    fn reconstruct_from_signed_digits(ss: &[i32], w: u32) -> [u64; LIMBS] {
        // 5 + 2 buffer limbs to absorb intermediate overflow before
        // propagating carries.
        let mut limbs = [0i128; LIMBS + 2];

        for (i, &d) in ss.iter().enumerate() {
            let shift = (i as u64) * u64::from(w);
            if shift >= ((LIMBS + 2) as u64) * 64 {
                continue;
            }
            let limb_idx = (shift / 64) as usize;
            let bit_off = (shift % 64) as u32;
            // Each digit lies in roughly `-(2^(w-1)+1) ..= 2^(w-1)+1` and
            // `bit_off <= 63`, so the shifted value fits in i128.
            let shifted = (d as i128) << bit_off;
            let lo_mask: i128 = (1i128 << 64) - 1;
            limbs[limb_idx] += shifted & lo_mask;
            if limb_idx + 1 < limbs.len() {
                limbs[limb_idx + 1] += shifted >> 64;
            }
        }
        let mut out = [0u64; LIMBS];
        let mut carry: i128 = 0;
        for (i, slot) in out.iter_mut().enumerate() {
            let v = limbs[i] + carry;
            *slot = v as u64;
            carry = v >> 64;
        }
        out
    }

    #[rstest]
    fn recode_signed_top_digit_is_nonnegative() {
        // Using the example from the upstream test, the top digit at index 32
        // (5-bit window over 5 limbs) is `-1` after carry propagation. The
        // 66-slot buffer covers the full 320-bit scalar (`64 * 5 = 320`) with
        // two slack slots so the reconstruction round-trip below has room for
        // any trailing carry.
        use crate::signing::field::MODULUS;

        let mut ss = [0i32; 66];
        let scalar = Scalar([
            MODULUS - 1,
            MODULUS - 2,
            MODULUS - 3,
            0xFFFF_FFFF_FFFF_FFFF,
            MODULUS - 5,
        ]);
        scalar.recode_signed(&mut ss, 5);

        assert_eq!(ss[6], -4);
        assert_eq!(ss[19], -2);
        assert_eq!(ss[25], -8);
        assert_eq!(ss[32], -1);

        // Stronger: the recoded digits reconstruct the original limbs via
        // `sum(ss[i] * 2^(w*i)) mod 2^320`.
        assert_eq!(reconstruct_from_signed_digits(&ss, 5), scalar.0);
    }

    proptest! {
        /// Modular addition is commutative.
        #[rstest]
        fn prop_add_commutative(a in arb_scalar(), b in arb_scalar()) {
            prop_assert_eq!(a + b, b + a);
        }

        /// Modular addition is associative.
        #[rstest]
        fn prop_add_associative(a in arb_scalar(), b in arb_scalar(), c in arb_scalar()) {
            prop_assert_eq!((a + b) + c, a + (b + c));
        }

        /// `a + (-a) == 0`.
        #[rstest]
        fn prop_neg_inverse(a in arb_scalar()) {
            prop_assert_eq!(a + (-a), Scalar::ZERO);
        }

        /// `(a + b) - b == a`.
        #[rstest]
        fn prop_sub_round_trip(a in arb_scalar(), b in arb_scalar()) {
            prop_assert_eq!((a + b) - b, a);
        }

        /// `a - b == a + (-b)`.
        #[rstest]
        fn prop_sub_via_add_neg(a in arb_scalar(), b in arb_scalar()) {
            prop_assert_eq!(a - b, a + (-b));
        }

        /// Modular multiplication is commutative.
        #[rstest]
        fn prop_mul_commutative(a in arb_scalar(), b in arb_scalar()) {
            prop_assert_eq!(a * b, b * a);
        }

        /// Modular multiplication is associative.
        #[rstest]
        fn prop_mul_associative(a in arb_scalar(), b in arb_scalar(), c in arb_scalar()) {
            prop_assert_eq!((a * b) * c, a * (b * c));
        }

        /// Multiplication distributes over addition.
        #[rstest]
        fn prop_distributive(a in arb_scalar(), b in arb_scalar(), c in arb_scalar()) {
            prop_assert_eq!(a * (b + c), a * b + a * c);
        }

        /// `Scalar::ONE` is the multiplicative identity.
        #[rstest]
        fn prop_one_is_identity(a in arb_scalar()) {
            prop_assert_eq!(a * Scalar::ONE, a);
            prop_assert_eq!(Scalar::ONE * a, a);
        }

        /// `Scalar::ZERO` annihilates multiplication.
        #[rstest]
        fn prop_zero_annihilates(a in arb_scalar()) {
            prop_assert_eq!(a * Scalar::ZERO, Scalar::ZERO);
            prop_assert_eq!(Scalar::ZERO * a, Scalar::ZERO);
        }

        /// `from_le_bytes_reduce(s.to_le_bytes()) == s` for any canonical
        /// scalar (idempotency under round-trip).
        #[rstest]
        fn prop_from_le_bytes_reduce_idempotent(s in arb_scalar()) {
            prop_assert_eq!(Scalar::from_le_bytes_reduce(s.to_le_bytes()), s);
        }

        /// Decoding any 40-byte sequence produces a canonical scalar.
        #[rstest]
        fn prop_from_le_bytes_reduce_yields_canonical(bytes in any::<[u8; SCALAR_BYTES]>()) {
            prop_assert!(Scalar::from_le_bytes_reduce(bytes).is_canonical());
        }

        /// `is_canonical` matches the hand-rolled limb compare against ORDER.
        #[rstest]
        fn prop_is_canonical_matches_lex_compare(s in arb_scalar()) {
            // Canonical iff `s < ORDER` lex-wise on the limb sequence.
            let mut expected = false;

            for i in (0..LIMBS).rev() {
                if s.0[i] < ORDER.0[i] {
                    expected = true;
                    break;
                }

                if s.0[i] > ORDER.0[i] {
                    expected = false;
                    break;
                }
            }
            prop_assert_eq!(s.is_canonical(), expected);
        }

        /// Scalar `select` picks branch by mask.
        #[rstest]
        fn prop_select_picks_branch(a in arb_scalar(), b in arb_scalar()) {
            prop_assert_eq!(Scalar::select(0, a, b), a);
            prop_assert_eq!(Scalar::select(u64::MAX, a, b), b);
        }

        /// `recode_signed` reconstructs the canonical 5-limb value via
        /// `sum(ss[i] * 2^(w*i)) mod 2^320` for every supported window width.
        #[rstest]
        fn prop_recode_signed_reconstructs(s in arb_scalar(), w in 2u32..=10) {
            // 320 bits divided by `w`, rounded up, plus 2 slack slots so the
            // final carry has somewhere to land regardless of window width.
            let len = (320usize.div_ceil(w as usize)) + 2;
            let mut ss = vec![0i32; len];
            s.recode_signed(&mut ss, w);
            prop_assert_eq!(reconstruct_from_signed_digits(&ss, w), s.0);

            // Each digit must lie in the signed window range.
            let bound: i32 = 1 << (w - 1);

            for (i, &d) in ss.iter().enumerate() {
                prop_assert!(
                    d >= -bound && d <= bound,
                    "digit {d} at index {i} outside [-{bound}, {bound}]",
                );
            }
        }

        /// `split_to_4_bit_limbs` and the test's reconstruction helper are
        /// inverses for any canonical scalar.
        #[rstest]
        fn prop_split_to_4_bit_limbs_round_trip(s in arb_scalar()) {
            prop_assert_eq!(reconstruct_from_4_bit_nibbles(s.split_to_4_bit_limbs()), s.0);
        }

        /// Every 4-bit nibble lies in `0..=15` for any scalar input.
        #[rstest]
        fn prop_split_to_4_bit_limbs_in_range(s in arb_scalar()) {
            for &nibble in &s.split_to_4_bit_limbs() {
                prop_assert!(nibble <= 15);
            }
        }
    }

    // `arb_scalar_nonzero` corollary: doubling a non-zero scalar lands on
    // `2 * s mod n`. Kept in its own block so consumers that import only
    // `arb_scalar` upstream don't pull the `nonzero` filter unnecessarily.
    proptest! {
        #[rstest]
        fn prop_double_via_add(s in arb_scalar_nonzero()) {
            prop_assert_eq!(s + s, s * Scalar::from_limbs([2, 0, 0, 0, 0]));
        }
    }
}
