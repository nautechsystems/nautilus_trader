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

//! ECgFp5 elliptic curve over `Fp5`.
//!
//! Implements the prime-order group `G ⊂ E(Fp5)` from Pornin's design
//! (eprint 2022/274), the curve underpinning Lighter's L2 Schnorr signer.
//! The curve equation is `y^2 = x*(x^2 + a*x + b)` with `a = 2`, `b = 263*z`.
//! The neutral element is the unique point of order 2 on the underlying
//! Weierstrass curve, `N = (0, 0)`; the encoding is the canonical `w = y/x`
//! mapping (computed as `t/u` from the fractional `(x, u) = (x, x/y)` form),
//! and decoding rejects malformed encodings.
//!
//! Internal representation uses fractional `(x, u) = (X/Z, U/T)` coordinates
//! so the addition formulas are complete (no special cases) at a cost of
//! `10M` per add. Doubling and `n`-fold doubling have specialized formulas
//! that share intermediate Frobenius-style products.
//!
//! Scalar multiplication uses a windowed signed-digit table of affine
//! `(x, u)` points with window width `5`. The window prep batches
//! `Fp5` inversions through Montgomery's trick, paying a single inversion for
//! the entire table. Both [`lookup`] and [`lookup_var_time`] mirror the
//! upstream Go behaviour: [`lookup_var_time`] short-circuits on the digit
//! and is suited to public-input top-window selection, while [`lookup`]
//! walks the table with a constant shape but assigns through a digit-derived
//! branch. [`Point::scalar_mul`] composes these and is therefore documented
//! variable-time, intended for verification-style operations where scalars
//! are public. The Schnorr signing path (secret nonce, secret key) routes
//! through [`Point::scalar_mul_ct`] instead, which uses the masked-select
//! [`lookup_ct`] for both the top digit and every inner step so neither
//! the table walk nor the final negation branches on the secret scalar.

use core::ops::{Add, AddAssign, Mul, Neg};
use std::sync::OnceLock;

use super::scalar::Scalar;
use crate::signing::field::{Fp, Fp5};

/// Curve coefficient `a = 2` (lifted into `Fp5`).
const A: Fp5 = Fp5([
    Fp::from_u64_reduce(2),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// Base scalar for `b`: `b = 263 * z` in `Fp5`. The factor `263` was chosen by
/// Pornin so that `(1, 1, 0, 0, 0)` lies on the curve (the canonical generator
/// is then `w = 4`).
const B1: u64 = 263;

/// Curve coefficient `b = 263 * z` (lifted into `Fp5`).
const B: Fp5 = Fp5([
    Fp::ZERO,
    Fp::from_u64_reduce(B1),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// `2 * b` precomputed for the addition formulas.
const B_MUL2: Fp5 = Fp5([
    Fp::ZERO,
    Fp::from_u64_reduce(2 * B1),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// `4 * b` precomputed for the doubling formulas.
const B_MUL4: Fp5 = Fp5([
    Fp::ZERO,
    Fp::from_u64_reduce(4 * B1),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// `16 * b` precomputed for the n-fold doubling tail.
const B_MUL16: Fp5 = Fp5([
    Fp::ZERO,
    Fp::from_u64_reduce(16 * B1),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// `4` lifted into `Fp5`, used as a curve-formula constant in `mdouble`.
const FOUR_FP5: Fp5 = Fp5([
    Fp::from_u64_reduce(4),
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
    Fp::ZERO,
]);

/// Window width for the precomputed multi-scalar table.
const WINDOW: u32 = 5;

/// Number of affine entries in the window table: `2^(WINDOW - 1)`.
const WIN_SIZE: usize = 1 << (WINDOW - 1);

/// Number of signed-digit slots required to span a 319-bit scalar at width
/// [`WINDOW`].
const NUM_DIGITS: usize = (319 + WINDOW as usize) / WINDOW as usize;

/// A point on the ECgFp5 curve in fractional `(x, u) = (X/Z, U/T)` coordinates.
///
/// The neutral element is encoded as `(0, 1, 0, 1)`. Two values are equal in
/// the group whenever `u1 * t2 == u2 * t1`; the [`Self::eq_point`] helper
/// performs that cross-multiplication without an inversion. `PartialEq` is
/// deliberately not derived because numerically distinct `(X, Z, U, T)` tuples
/// can represent the same group element.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    /// `X` coordinate of the projective `(X, Z)` representation.
    pub x: Fp5,
    /// `Z` denominator of the `(X, Z)` representation.
    pub z: Fp5,
    /// `U` coordinate of the projective `(U, T)` representation.
    pub u: Fp5,
    /// `T` denominator of the `(U, T)` representation.
    pub t: Fp5,
}

/// A point on the curve in affine `(x, u)` coordinates. Used as window entries
/// in scalar multiplication; the affine form skips the `Z` and `T` denominators.
#[derive(Clone, Copy, Debug)]
pub struct AffinePoint {
    /// Affine `x` coordinate.
    pub x: Fp5,
    /// Affine `u` coordinate (`u = x / y`).
    pub u: Fp5,
}

impl AffinePoint {
    /// Affine encoding of the neutral element `N = (0, 0)`.
    pub const NEUTRAL: Self = Self {
        x: Fp5::ZERO,
        u: Fp5::ZERO,
    };

    /// Lift to fractional `(X, Z, U, T)` coordinates with `Z = T = 1`.
    #[must_use]
    pub fn to_point(self) -> Point {
        Point {
            x: self.x,
            z: Fp5::ONE,
            u: self.u,
            t: Fp5::ONE,
        }
    }

    /// Negate the point in place by negating the `u` coordinate.
    pub fn set_neg(&mut self) {
        self.u = -self.u;
    }
}

impl Neg for AffinePoint {
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            x: self.x,
            u: -self.u,
        }
    }
}

impl Point {
    /// Neutral element `N = (0, 0)` in projective `(X, Z, U, T) = (0, 1, 0, 1)`.
    pub const NEUTRAL: Self = Self {
        x: Fp5::ZERO,
        z: Fp5::ONE,
        u: Fp5::ZERO,
        t: Fp5::ONE,
    };

    /// Canonical generator of the prime-order subgroup, `w = 4`.
    pub const GENERATOR: Self = Self {
        x: Fp5([
            Fp::from_u64_reduce(12_883_135_586_176_881_569),
            Fp::from_u64_reduce(4_356_519_642_755_055_268),
            Fp::from_u64_reduce(5_248_930_565_894_896_907),
            Fp::from_u64_reduce(2_165_973_894_480_315_022),
            Fp::from_u64_reduce(2_448_410_071_095_648_785),
        ]),
        z: Fp5::ONE,
        u: Fp5::ONE,
        t: Fp5([
            Fp::from_u64_reduce(4),
            Fp::ZERO,
            Fp::ZERO,
            Fp::ZERO,
            Fp::ZERO,
        ]),
    };

    /// Group equality: returns `true` when `self` and `rhs` denote the same
    /// curve point. Implemented as `u1 * t2 == u2 * t1` to avoid inversion.
    #[must_use]
    pub fn eq_point(self, rhs: Self) -> bool {
        self.u * rhs.t == rhs.u * self.t
    }

    /// Test whether the point is the neutral element `N`. The neutral has
    /// `u == 0` in the fractional encoding.
    #[inline]
    #[must_use]
    pub fn is_neutral(self) -> bool {
        self.u.is_zero()
    }

    /// Encode the point to its canonical 40-byte little-endian `Fp5`
    /// representation `w = t / u`. The neutral encodes to `0`.
    #[must_use]
    pub fn encode(self) -> Fp5 {
        self.t * self.u.invert()
    }

    /// Decode a curve point from its canonical `w = y/x` encoding.
    ///
    /// Returns `Some(point)` for any element of the prime-order group (the
    /// zero encoding maps to [`Self::NEUTRAL`]). Returns `None` for encodings
    /// that do not correspond to a valid group element. Decoding succeeds iff
    /// `w == 0` or `(w^2 - a)^2 - 4*b` is a quadratic residue in `Fp5`.
    #[must_use]
    pub fn decode(w: Fp5) -> Option<Self> {
        let e = w.square() - A;
        let delta = e.square() - B_MUL4;
        let r_opt = delta.canonical_sqrt();
        let success = r_opt.is_some();
        let r = r_opt.unwrap_or(Fp5::ZERO);

        if !success {
            // Per Pornin: when delta is not a square, the only valid encoding
            // is `w = 0`, which represents the neutral. Anything else fails.
            return if w.is_zero() {
                Some(Self::NEUTRAL)
            } else {
                None
            };
        }

        let two_inv = Fp5::from_u64s_reduce([2, 0, 0, 0, 0]).invert();
        let x1 = (e + r) * two_inv;
        let x2 = (e - r) * two_inv;
        // Pick the candidate whose Legendre symbol is `-1` (non-square): that
        // is the unique pre-image under the encoding.
        let x = if x1.legendre() == Fp::ONE { x2 } else { x1 };

        Some(Self {
            x,
            z: Fp5::ONE,
            u: Fp5::ONE,
            t: w,
        })
    }

    /// Group addition `self ⊕ rhs` via the complete `10M` formulas of
    /// Pornin's paper. Handles all combinations including the neutral.
    #[must_use]
    pub fn add_point(self, rhs: Self) -> Self {
        let (x1, z1, u1, t1_) = (self.x, self.z, self.u, self.t);
        let (x2, z2, u2, t2_) = (rhs.x, rhs.z, rhs.u, rhs.t);

        let t1 = x1 * x2;
        let t2 = z1 * z2;
        let t3 = u1 * u2;
        let t4 = t1_ * t2_;
        let t5 = (x1 + z1) * (x2 + z2) - t1 - t2;
        let t6 = (u1 + t1_) * (u2 + t2_) - t3 - t4;
        let t7 = t1 + t2 * B;
        let t8 = t4 * t7;
        let t9 = t3 * (t5 * B_MUL2 + t7.double());
        let t10 = (t4 + t3.double()) * (t5 + t7);

        let x_new = (t10 - t8) * B;
        let z_new = t8 - t9;
        let u_new = t6 * (t2 * B - t1);
        let t_new = t8 + t9;

        Self {
            x: x_new,
            z: z_new,
            u: u_new,
            t: t_new,
        }
    }

    /// Add an affine `(x, u)` point. Cost: `8M` (two fewer multiplies than the
    /// general add since `z2 == t2 == 1`).
    #[must_use]
    pub fn add_affine(self, rhs: AffinePoint) -> Self {
        let (x1, z1, u1, t1_) = (self.x, self.z, self.u, self.t);
        let (x2, u2) = (rhs.x, rhs.u);

        let t1 = x1 * x2;
        let t2 = z1;
        let t3 = u1 * u2;
        let t4 = t1_;
        let t5 = x1 + x2 * z1;
        let t6 = u1 + u2 * t1_;
        let t7 = t1 + t2 * B;
        let t8 = t4 * t7;
        let t9 = t3 * (t5 * B_MUL2 + t7.double());
        let t10 = (t4 + t3.double()) * (t5 + t7);

        Self {
            x: (t10 - t8) * B,
            u: t6 * (t2 * B - t1),
            z: t8 - t9,
            t: t8 + t9,
        }
    }

    /// Group doubling `2 * self`. Cost: `4M + 5S`.
    #[must_use]
    pub fn double(self) -> Self {
        let mut p = self;
        p.set_double();
        p
    }

    /// In-place doubling. Splitting `Self::double` into a setter saves a
    /// struct copy in the inner loop of `mdouble`.
    pub fn set_double(&mut self) {
        let x = self.x;
        let z = self.z;
        let u = self.u;
        let t = self.t;

        let t1 = z * t;
        let t2 = t1 * t;
        let x1 = t2.square();
        let z1 = t1 * u;
        let t3 = u.square();
        let w1 = t2 - t3 * (x + z).double();
        let t4 = z1.square();

        let x_new = t4 * B_MUL4;
        let z_new = w1.square();
        let u_new = (w1 + z1).square() - t4 - z_new;
        let t_new = x1.double() - (t4 * FOUR_FP5 + z_new);

        self.x = x_new;
        self.z = z_new;
        self.u = u_new;
        self.t = t_new;
    }

    /// `n`-fold doubling, returning a new point. For `n >= 2`, uses a
    /// share-the-doubling formulation that avoids reconstructing the
    /// intermediate `(X, Z, U, T)` tuple between rounds.
    #[must_use]
    pub fn mdouble(self, n: u32) -> Self {
        let mut p = self;
        p.set_mdouble(n);
        p
    }

    /// In-place `n`-fold doubling. Cost: `n * (2M + 5S) + 2M + 1S`.
    pub fn set_mdouble(&mut self, n: u32) {
        if n == 0 {
            return;
        }

        if n == 1 {
            self.set_double();
            return;
        }

        let x0 = self.x;
        let z0 = self.z;
        let u0 = self.u;
        let t0 = self.t;

        let t1 = z0 * t0;
        let t2 = t1 * t0;
        let x1 = t2.square();
        let z1 = t1 * u0;
        let t3 = u0.square();
        let w1 = t2 - (x0 + z0).double() * t3;
        let t4 = w1.square();
        let t5 = z1.square();

        let mut x_state = t5.square() * B_MUL16;
        let mut w_state = x1.double() - (t5 * FOUR_FP5 + t4);
        let mut z_state = (w1 + z1).square() - t4 - t5;

        for _ in 2..n {
            mdouble_inner_round(&mut x_state, &mut w_state, &mut z_state);
        }

        let t1f = w_state.square();
        let t2f = z_state.square();
        let t3f = (w_state + z_state).square() - t1f - t2f;
        let w1f = t1f - (x_state + t2f).double();

        let z_out = w1f.square();
        self.x = t3f.square() * B;
        self.z = z_out;
        self.u = t3f * w1f;
        self.t = t1f.double() * (t1f - t2f.double()) - z_out;
    }

    /// Build a `WINDOW`-bit window of affine multiples
    /// `[1*P, 2*P, ..., 2^(WINDOW-1) * P]` for the supplied base point.
    #[must_use]
    pub fn make_window_affine(self) -> Vec<AffinePoint> {
        let mut tmp = Vec::with_capacity(WIN_SIZE);
        tmp.push(self);

        for i in 1..WIN_SIZE {
            if (i & 1) == 0 {
                let last = tmp[i - 1];
                tmp.push(last.add_point(self));
            } else {
                let half = tmp[i >> 1];
                tmp.push(half.double());
            }
        }
        batch_to_affine(&tmp)
    }

    /// Variable-time scalar multiplication `s * self`.
    ///
    /// Builds a windowed table of `(x, u)` affine multiples and performs a
    /// signed-digit double-and-add. This routine MUST NOT be called on secret
    /// scalars: the top-window [`lookup_var_time`] step short-circuits on the
    /// digit, and the per-window [`lookup`] still branches on each entry's
    /// match (see its doc). Use [`Self::scalar_mul_ct`] for the secret-scalar
    /// path that the Schnorr signer follows.
    #[must_use]
    pub fn scalar_mul(self, s: Scalar) -> Self {
        let win = self.make_window_affine();
        let mut digits = [0i32; NUM_DIGITS];
        s.recode_signed(&mut digits, WINDOW);
        scalar_mul_with_window_var_time(&win, &digits)
    }

    /// Constant-time scalar multiplication `s * self`.
    ///
    /// Same algorithm as [`Self::scalar_mul`] but routes every window lookup
    /// through [`lookup_ct`], which uses [`Fp5::ct_select`] in place of the
    /// data-dependent assignment in [`lookup`]. The window-prep, recoding,
    /// `mdouble`, and `add_affine` steps execute as straight-line fixed-shape
    /// sequences over the secret scalar's limbs, so the routine leaks no
    /// timing information about `s`. The base point `self` is treated as
    /// public; this is the form Schnorr uses (`k * G`, `sk * G`).
    #[must_use]
    pub fn scalar_mul_ct(self, s: Scalar) -> Self {
        let win = self.make_window_affine();
        let mut digits = [0i32; NUM_DIGITS];
        s.recode_signed(&mut digits, WINDOW);
        scalar_mul_with_window_ct(&win, &digits)
    }

    /// Constant-time scalar multiplication on the canonical generator
    /// `s * G`, using a precomputed affine window cached for the process
    /// lifetime.
    ///
    /// Functionally identical to `Point::GENERATOR.scalar_mul_ct(s)` but skips
    /// the per-call [`Self::make_window_affine`] step. The Schnorr signer's
    /// `r = k * G` and `pk = sk * G` derivations both route through here.
    #[must_use]
    pub fn mulgen_ct(s: Scalar) -> Self {
        let win = generator_window();
        let mut digits = [0i32; NUM_DIGITS];
        s.recode_signed(&mut digits, WINDOW);
        scalar_mul_with_window_ct(win, &digits)
    }

    /// Variable-time scalar multiplication on the canonical generator `s * G`.
    ///
    /// Companion to [`Self::mulgen_ct`] for verification's `s * G` term, where
    /// the scalar is public. Reuses the same precomputed affine window.
    #[must_use]
    pub fn mulgen(s: Scalar) -> Self {
        let win = generator_window();
        let mut digits = [0i32; NUM_DIGITS];
        s.recode_signed(&mut digits, WINDOW);
        scalar_mul_with_window_var_time(win, &digits)
    }
}

// Shared variable-time loop body. Factored out so per-base scalar_mul and
// the precomputed-generator mulgen path can share digit recoding.
fn scalar_mul_with_window_var_time(win: &[AffinePoint], digits: &[i32; NUM_DIGITS]) -> Point {
    let mut p = lookup_var_time(win, digits[NUM_DIGITS - 1]).to_point();
    for i in (0..NUM_DIGITS - 1).rev() {
        p.set_mdouble(WINDOW);
        let entry = lookup(win, digits[i]);
        p = p.add_affine(entry);
    }
    p
}

// Constant-time companion: every lookup routes through `lookup_ct` so the
// table walk and final negation do not branch on the secret digit.
fn scalar_mul_with_window_ct(win: &[AffinePoint], digits: &[i32; NUM_DIGITS]) -> Point {
    let mut p = lookup_ct(win, digits[NUM_DIGITS - 1]).to_point();
    for i in (0..NUM_DIGITS - 1).rev() {
        p.set_mdouble(WINDOW);
        let entry = lookup_ct(win, digits[i]);
        p = p.add_affine(entry);
    }
    p
}

// Affine multiples of the canonical generator, lazily initialised on first
// use and shared across every `mulgen` / `mulgen_ct` call in the process.
fn generator_window() -> &'static [AffinePoint; WIN_SIZE] {
    static WINDOW_CACHE: OnceLock<[AffinePoint; WIN_SIZE]> = OnceLock::new();
    WINDOW_CACHE.get_or_init(|| {
        let v = Point::GENERATOR.make_window_affine();
        let mut out = [AffinePoint::NEUTRAL; WIN_SIZE];
        for (slot, entry) in out.iter_mut().zip(v.iter()) {
            *slot = *entry;
        }
        out
    })
}

/// Inner round of [`Point::set_mdouble`] for indices `2..n`. Mutates the `x`,
/// `w`, `z` carry triple in place.
fn mdouble_inner_round(x: &mut Fp5, w: &mut Fp5, z: &mut Fp5) {
    let t1 = z.square();
    let t2 = t1.square();
    let t3 = w.square();
    let t4 = t3.square();
    let t5 = (*w + *z).square() - t1 - t3;

    *z = t5 * ((*x + t1).double() - t3);
    *x = t2 * t4 * B_MUL16;
    *w = -(t4 + t2 * (B_MUL4 - FOUR_FP5));
}

/// Convert a slice of projective points to affine `(x, u)` form using
/// Montgomery's trick: a single `Fp5` inversion services all entries.
#[must_use]
pub fn batch_to_affine(src: &[Point]) -> Vec<AffinePoint> {
    let n = src.len();
    if n == 0 {
        return Vec::new();
    }

    if n == 1 {
        let p = src[0];
        let m1 = (p.z * p.t).invert();
        return vec![AffinePoint {
            x: p.x * p.t * m1,
            u: p.u * p.z * m1,
        }];
    }

    let mut res = vec![
        AffinePoint {
            x: Fp5::ZERO,
            u: Fp5::ZERO,
        };
        n
    ];
    let mut m = src[0].z * src[0].t;

    for i in 1..n {
        let x_partial = m;
        m *= src[i].z;
        let u_partial = m;
        m *= src[i].t;

        res[i] = AffinePoint {
            x: x_partial,
            u: u_partial,
        };
    }

    m = m.invert();

    for i in (1..n).rev() {
        res[i].u = src[i].u * res[i].u * m;
        m *= src[i].t;
        res[i].x = src[i].x * res[i].x * m;
        m *= src[i].z;
    }
    res[0].u = src[0].u * src[0].z * m;
    m *= src[0].t;
    res[0].x = src[0].x * m;

    res
}

/// Window lookup: returns the affine point matching the signed digit `k`.
///
/// `k > 0` selects `k * P`, `k < 0` selects `-k * P`, and `k == 0` returns
/// the affine neutral `(0, 0)`. Mirrors the upstream Go `Lookup` byte-for-byte.
///
/// Despite the constant-shape walk over `win`, both the per-entry copy and
/// the final negation branch on the digit's value, so secret digits leak
/// through timing. A true constant-time variant requires masked-select Fp5
/// primitives and lands in Phase D alongside the secret-scalar Schnorr path.
#[must_use]
pub fn lookup(win: &[AffinePoint], k: i32) -> AffinePoint {
    let sign = (k >> 31) as u32;
    let ka = ((k as u32) ^ sign).wrapping_sub(sign);
    let km1 = ka.wrapping_sub(1);

    let mut x = Fp5::ZERO;
    let mut u = Fp5::ZERO;

    for (i, entry) in win.iter().enumerate() {
        let m = km1.wrapping_sub(i as u32);
        let c1 = (m | (!m).wrapping_add(1)) >> 31;
        let c = (u64::from(c1)).wrapping_sub(1);
        if c != 0 {
            x = entry.x;
            u = entry.u;
        }
    }

    let neg_mask = u64::from(sign) | (u64::from(sign) << 32);
    if neg_mask != 0 {
        u = -u;
    }
    AffinePoint { x, u }
}

/// Constant-time window lookup. Same semantics as [`lookup`] but every
/// per-entry assignment and the final `u` negation route through
/// [`Fp5::ct_select`], so neither the table walk nor the sign branch leaks
/// the secret digit. Used by [`Point::scalar_mul_ct`].
#[must_use]
pub fn lookup_ct(win: &[AffinePoint], k: i32) -> AffinePoint {
    let sign = (k >> 31) as u32;
    let ka = ((k as u32) ^ sign).wrapping_sub(sign);
    let km1 = ka.wrapping_sub(1);

    let mut x = Fp5::ZERO;
    let mut u = Fp5::ZERO;

    for (i, entry) in win.iter().enumerate() {
        let m = km1.wrapping_sub(i as u32);
        let c1 = (m | (!m).wrapping_add(1)) >> 31;
        let mask = u64::from(c1).wrapping_sub(1);
        x = Fp5::ct_select(mask, x, entry.x);
        u = Fp5::ct_select(mask, u, entry.u);
    }

    let neg_mask = sign as i32 as i64 as u64;
    let neg_u = -u;
    u = Fp5::ct_select(neg_mask, u, neg_u);
    AffinePoint { x, u }
}

/// Variable-time window lookup. Same semantics as [`lookup`] but with explicit
/// short-circuit branches; only suitable for public-input scalar multiplication.
#[must_use]
pub fn lookup_var_time(win: &[AffinePoint], k: i32) -> AffinePoint {
    if k == 0 {
        AffinePoint::NEUTRAL
    } else if k > 0 {
        win[(k - 1) as usize]
    } else {
        let mut res = win[(-k - 1) as usize];
        res.set_neg();
        res
    }
}

impl Add for Point {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        self.add_point(rhs)
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.add_point(rhs);
    }
}

impl Mul<Scalar> for Point {
    type Output = Self;

    fn mul(self, rhs: Scalar) -> Self {
        self.scalar_mul(rhs)
    }
}

impl Mul<Point> for Scalar {
    type Output = Point;

    fn mul(self, rhs: Point) -> Point {
        rhs.scalar_mul(self)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;
    use serde::Deserialize;

    use super::*;
    use crate::signing::fixtures::{
        arb_point, arb_scalar, bytes_to_hex, decode_fp5_bytes, hex_to_bytes,
    };

    const VECTORS_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_data/signing_curve_ecgfp5_vectors.json",
    ));

    #[derive(Debug, Deserialize)]
    struct VectorsFile {
        vectors: Vectors,
    }

    #[derive(Debug, Deserialize)]
    struct Vectors {
        decode: Vec<DecodeVector>,
        add: Vec<AddVector>,
        scalar_mul: Vec<ScalarMulVector>,
        scalar_ops: Vec<ScalarOpVector>,
    }

    #[derive(Debug, Deserialize)]
    struct DecodeVector {
        w: String,
        decodes: bool,
        is_neutral: bool,
        encoded_back: String,
    }

    #[derive(Debug, Deserialize)]
    struct AddVector {
        a_w: String,
        b_w: String,
        sum_w: String,
        a_double_w: String,
    }

    #[derive(Debug, Deserialize)]
    struct ScalarMulVector {
        base_w: String,
        scalar_le: String,
        out_w: String,
    }

    #[derive(Debug, Deserialize)]
    struct ScalarOpVector {
        a: String,
        b: String,
        add: String,
        sub: String,
        mul: String,
        neg_a: String,
        a_is_zero: bool,
    }

    fn decode_scalar(hex: &str) -> Scalar {
        let bytes = hex_to_bytes(hex);
        let mut buf = [0u8; super::super::scalar::SCALAR_BYTES];
        buf.copy_from_slice(&bytes);
        Scalar::from_le_bytes_reduce(buf)
    }

    fn fp5(c: [u64; 5]) -> Fp5 {
        Fp5::from_u64s_reduce(c)
    }

    #[rstest]
    fn neutral_decodes_from_zero() {
        let p = Point::decode(Fp5::ZERO).expect("zero decodes to neutral");
        assert!(p.is_neutral());
    }

    #[rstest]
    fn generator_is_canonical() {
        let g = Point::GENERATOR;
        assert_eq!(g.encode(), fp5([4, 0, 0, 0, 0]));
    }

    #[rstest]
    fn neutral_encodes_to_zero() {
        // The encoding is t/u, which is 1/0 = 0 by the InverseOrZero convention.
        assert_eq!(Point::NEUTRAL.encode(), Fp5::ZERO);
    }

    #[rstest]
    fn add_with_neutral_is_identity() {
        let g = Point::GENERATOR;
        let g_plus_n = g.add_point(Point::NEUTRAL);
        assert!(g_plus_n.eq_point(g));

        let n_plus_g = Point::NEUTRAL.add_point(g);
        assert!(n_plus_g.eq_point(g));
    }

    #[rstest]
    fn double_matches_self_addition() {
        let g = Point::GENERATOR;
        let g2 = g.double();
        let g_plus_g = g.add_point(g);
        assert!(g2.eq_point(g_plus_g));
    }

    #[rstest]
    fn mdouble_matches_iterated_double() {
        let g = Point::GENERATOR;

        for n in 0..10u32 {
            let mut iter = g;
            for _ in 0..n {
                iter = iter.double();
            }
            let bulk = g.mdouble(n);
            assert!(
                iter.eq_point(bulk),
                "mdouble({n}) diverged from iterated double"
            );
        }
    }

    #[rstest]
    fn add_affine_matches_general_add() {
        let g = Point::GENERATOR;
        let g2 = g.double();
        let g2_affine = AffinePoint {
            x: g2.x * g2.z.invert(),
            u: g2.u * g2.t.invert(),
        };
        let sum_affine = g.add_affine(g2_affine);
        let sum_point = g.add_point(g2);
        assert!(sum_affine.eq_point(sum_point));
    }

    #[rstest]
    fn scalar_one_is_identity_under_mul() {
        let g = Point::GENERATOR;
        let g1 = g * Scalar::ONE;
        assert!(g1.eq_point(g));
    }

    #[rstest]
    fn scalar_mul_ct_matches_scalar_mul() {
        let g = Point::GENERATOR;
        // Top-window stress: `ORDER - 1` exercises the topmost signed digit
        // and forces non-trivial carries through the recoding window.
        let mut order_minus_one = super::super::scalar::ORDER.to_limbs();
        order_minus_one[0] -= 1;

        let scalars = [
            Scalar::ZERO,
            Scalar::ONE,
            Scalar::from_limbs([2, 0, 0, 0, 0]),
            Scalar::from_limbs([0xDEAD_BEEF, 0, 0, 0, 0]),
            Scalar::from_limbs([
                0x0123_4567_89AB_CDEF,
                0xFEDC_BA98_7654_3210,
                0x1111_2222_3333_4444,
                0x5555_6666_7777_8888,
                0x0000_0001_0000_0001,
            ]),
            Scalar::from_limbs(order_minus_one),
        ];

        for (i, s) in scalars.iter().enumerate() {
            let var = g.scalar_mul(*s);
            let ct = g.scalar_mul_ct(*s);
            assert!(var.eq_point(ct), "scalar {i}: var-time vs CT diverged");
            assert_eq!(
                var.encode().to_le_bytes(),
                ct.encode().to_le_bytes(),
                "scalar {i}: encoded outputs diverged",
            );
        }
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(-1)]
    #[case(2)]
    #[case(-2)]
    #[case(7)]
    #[case(-7)]
    #[case(8)]
    #[case(-8)]
    #[case(15)]
    #[case(-15)]
    #[case(16)]
    #[case(-16)]
    fn lookup_ct_matches_lookup(#[case] k: i32) {
        let win = Point::GENERATOR.make_window_affine();
        let var = lookup(&win, k);
        let ct = lookup_ct(&win, k);
        assert_eq!(var.x, ct.x, "k={k}: x coordinate diverged");
        assert_eq!(var.u, ct.u, "k={k}: u coordinate diverged");
    }

    #[rstest]
    fn batch_to_affine_handles_empty() {
        let out = batch_to_affine(&[]);
        assert!(out.is_empty(), "empty input must produce empty output");
    }

    #[rstest]
    fn batch_to_affine_handles_single() {
        let g2 = Point::GENERATOR.double();
        let out = batch_to_affine(&[g2]);
        assert_eq!(out.len(), 1);
        // Compare against the naive (x * z.invert(), u * t.invert()).
        let expected = AffinePoint {
            x: g2.x * g2.z.invert(),
            u: g2.u * g2.t.invert(),
        };
        assert_eq!(out[0].x, expected.x, "single-batch x diverged");
        assert_eq!(out[0].u, expected.u, "single-batch u diverged");
    }

    /// `ORDER * G == NEUTRAL`: the group order kills every point. Computed
    /// as `(ORDER - 1) * G + G` to avoid passing a non-canonical scalar
    /// through `scalar_mul`.
    #[rstest]
    fn order_times_generator_is_neutral() {
        let mut order_minus_one_limbs = super::super::scalar::ORDER.to_limbs();
        order_minus_one_limbs[0] -= 1;
        let order_minus_one = Scalar::from_limbs(order_minus_one_limbs);

        let neg_g = Point::GENERATOR * order_minus_one;
        let total = neg_g.add_point(Point::GENERATOR);
        assert!(total.is_neutral(), "(ORDER - 1) * G + G must equal NEUTRAL",);
    }

    /// `(ORDER - 1) * G == -G` (additive inverse).
    #[rstest]
    fn order_minus_one_times_generator_is_neg_generator() {
        let mut order_minus_one_limbs = super::super::scalar::ORDER.to_limbs();
        order_minus_one_limbs[0] -= 1;
        let order_minus_one = Scalar::from_limbs(order_minus_one_limbs);

        let neg_g = Point::GENERATOR * order_minus_one;
        // Adding `-G` to `G` lands on the neutral.
        let identity = neg_g.add_point(Point::GENERATOR);
        assert!(identity.is_neutral());
    }

    proptest! {
        /// `scalar_mul_ct` and `scalar_mul` MUST produce the same group
        /// element on every canonical scalar and every base point. Encoded
        /// outputs MUST also be byte-identical.
        #[rstest]
        fn prop_scalar_mul_ct_matches_scalar_mul(
            base in arb_point(),
            s in arb_scalar(),
        ) {
            let var = base.scalar_mul(s);
            let ct = base.scalar_mul_ct(s);
            prop_assert!(var.eq_point(ct));
            prop_assert_eq!(var.encode().to_le_bytes(), ct.encode().to_le_bytes());
        }

        /// `lookup` and `lookup_ct` agree on every signed digit in the
        /// window range `[-WIN_SIZE, WIN_SIZE]` (`WIN_SIZE = 16`).
        #[rstest]
        fn prop_lookup_ct_matches_lookup(k in -16i32..=16) {
            let win = Point::GENERATOR.make_window_affine();
            let var = lookup(&win, k);
            let ct = lookup_ct(&win, k);
            prop_assert_eq!(var.x, ct.x);
            prop_assert_eq!(var.u, ct.u);
        }

        /// `Point::mulgen_ct(s)` matches `Point::GENERATOR.scalar_mul_ct(s)`
        /// byte-for-byte on every canonical scalar. Pins the precomputed
        /// generator window against the per-call window-prep path.
        #[rstest]
        fn prop_mulgen_ct_matches_scalar_mul_ct(s in arb_scalar()) {
            let baseline = Point::GENERATOR.scalar_mul_ct(s);
            let cached = Point::mulgen_ct(s);
            prop_assert!(baseline.eq_point(cached));
            prop_assert_eq!(
                baseline.encode().to_le_bytes(),
                cached.encode().to_le_bytes(),
            );
        }

        /// `Point::mulgen(s)` matches `Point::GENERATOR.scalar_mul(s)`
        /// byte-for-byte on every canonical scalar.
        #[rstest]
        fn prop_mulgen_matches_scalar_mul(s in arb_scalar()) {
            let baseline = Point::GENERATOR.scalar_mul(s);
            let cached = Point::mulgen(s);
            prop_assert!(baseline.eq_point(cached));
            prop_assert_eq!(
                baseline.encode().to_le_bytes(),
                cached.encode().to_le_bytes(),
            );
        }
    }

    proptest! {
        // Group-law and round-trip properties. These run scalar_mul once or
        // twice per case, so we keep cases at the proptest default (256).

        /// Group addition is commutative: `a + b == b + a`.
        #[rstest]
        fn prop_add_commutative(a in arb_point(), b in arb_point()) {
            prop_assert!(a.add_point(b).eq_point(b.add_point(a)));
        }

        /// Adding the neutral element is the identity.
        #[rstest]
        fn prop_neutral_is_identity(p in arb_point()) {
            prop_assert!(p.add_point(Point::NEUTRAL).eq_point(p));
            prop_assert!(Point::NEUTRAL.add_point(p).eq_point(p));
        }

        /// `p + p == p.double()`.
        #[rstest]
        fn prop_double_via_add(p in arb_point()) {
            prop_assert!(p.add_point(p).eq_point(p.double()));
        }

        /// `add_affine` matches `add_point` after lifting the second operand
        /// to affine via inversion.
        #[rstest]
        fn prop_add_affine_matches_general_add(p in arb_point(), q in arb_point()) {
            // Skip cases where q is the neutral: `t == 1, u == 0` makes the
            // affine lift undefined (division by zero through `u.invert()`).
            prop_assume!(!q.is_neutral());
            let q_affine = AffinePoint {
                x: q.x * q.z.invert(),
                u: q.u * q.t.invert(),
            };
            prop_assert!(p.add_affine(q_affine).eq_point(p.add_point(q)));
        }

        /// `Point::decode(p.encode()) == p` for every group point.
        #[rstest]
        fn prop_encode_decode_round_trip(p in arb_point()) {
            let w = p.encode();
            let p2 = Point::decode(w).expect("encoded group point must decode");
            prop_assert!(p2.eq_point(p));
        }

        /// `Scalar::ONE * p == p`.
        #[rstest]
        fn prop_scalar_one_is_identity(p in arb_point()) {
            prop_assert!((p * Scalar::ONE).eq_point(p));
        }

        /// `Scalar::ZERO * p == NEUTRAL`.
        #[rstest]
        fn prop_scalar_zero_kills_point(p in arb_point()) {
            prop_assert!((p * Scalar::ZERO).is_neutral());
        }
    }

    proptest! {
        // Heavier algebraic identities; each case runs 2-3 scalar muls.
        // 64 cases is enough to catch systematic regressions while keeping
        // total runtime bounded.
        #![proptest_config(ProptestConfig {
            cases: 64,
            ..ProptestConfig::default()
        })]

        /// Group addition is associative.
        #[rstest]
        fn prop_add_associative(a in arb_point(), b in arb_point(), c in arb_point()) {
            let lhs = a.add_point(b).add_point(c);
            let rhs = a.add_point(b.add_point(c));
            prop_assert!(lhs.eq_point(rhs));
        }

        /// Scalar multiplication distributes over scalar addition:
        /// `(a + b) * P == a * P + b * P`.
        #[rstest]
        fn prop_scalar_mul_distributive(
            a in arb_scalar(),
            b in arb_scalar(),
            p in arb_point(),
        ) {
            let lhs = p * (a + b);
            let rhs = (p * a).add_point(p * b);
            prop_assert!(lhs.eq_point(rhs));
        }

        /// Scalar multiplication is associative:
        /// `a * (b * P) == (a * b) * P`.
        #[rstest]
        fn prop_scalar_mul_associative(
            a in arb_scalar(),
            b in arb_scalar(),
            p in arb_point(),
        ) {
            let lhs = (p * b) * a;
            let rhs = p * (a * b);
            prop_assert!(lhs.eq_point(rhs));
        }

        /// `mdouble(n)` matches `n` iterated `double` calls for small `n`.
        #[rstest]
        fn prop_mdouble_via_double(p in arb_point(), n in 0u32..6) {
            let mut iter = p;
            for _ in 0..n {
                iter = iter.double();
            }
            prop_assert!(p.mdouble(n).eq_point(iter));
        }

        /// `batch_to_affine` produces the same `(x, u)` as the naive
        /// per-point `(x * z.invert(), u * t.invert())` for any small batch.
        #[rstest]
        fn prop_batch_to_affine_matches_naive(
            seeds in proptest::collection::vec(arb_scalar(), 1..8),
        ) {
            let pts: Vec<Point> = seeds
                .iter()
                .map(|s| Point::GENERATOR.scalar_mul(*s))
                .filter(|p| !p.is_neutral())
                .collect();
            prop_assume!(!pts.is_empty());

            let batched = batch_to_affine(&pts);
            prop_assert_eq!(batched.len(), pts.len());

            for (i, p) in pts.iter().enumerate() {
                let expected = AffinePoint {
                    x: p.x * p.z.invert(),
                    u: p.u * p.t.invert(),
                };
                prop_assert_eq!(batched[i].x, expected.x, "x at {} diverged", i);
                prop_assert_eq!(batched[i].u, expected.u, "u at {} diverged", i);
            }
        }
    }

    #[rstest]
    fn scalar_two_matches_double() {
        let g = Point::GENERATOR;
        let g2 = g * Scalar::from_limbs([2, 0, 0, 0, 0]);
        assert!(g2.eq_point(g.double()));
    }

    #[rstest]
    fn add_is_commutative() {
        let g = Point::GENERATOR;
        let g3 = g.double().add_point(g);
        let g3_alt = g.add_point(g.double());
        assert!(g3.eq_point(g3_alt));
    }

    #[rstest]
    fn decode_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.decode.is_empty(), "decode vectors empty");

        for (i, v) in suite.vectors.decode.iter().enumerate() {
            let w = decode_fp5_bytes(&v.w);
            let decoded = Point::decode(w);

            assert_eq!(decoded.is_some(), v.decodes, "vector {i}: decode success");

            if let Some(p) = decoded {
                assert_eq!(p.is_neutral(), v.is_neutral, "vector {i}: is_neutral");

                let encoded = p.encode();
                assert_eq!(
                    bytes_to_hex(&encoded.to_le_bytes()),
                    v.encoded_back,
                    "vector {i}: re-encode round trip",
                );
            }
        }
    }

    #[rstest]
    fn add_and_double_match_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(!suite.vectors.add.is_empty(), "add vectors empty");

        for (i, v) in suite.vectors.add.iter().enumerate() {
            let a_w = decode_fp5_bytes(&v.a_w);
            let b_w = decode_fp5_bytes(&v.b_w);
            let a = Point::decode(a_w).unwrap_or_else(|| panic!("vector {i}: decode a"));
            let b = Point::decode(b_w).unwrap_or_else(|| panic!("vector {i}: decode b"));

            let sum = a.add_point(b);
            assert_eq!(
                bytes_to_hex(&sum.encode().to_le_bytes()),
                v.sum_w,
                "vector {i}: a + b",
            );

            let double_a = a.double();
            assert_eq!(
                bytes_to_hex(&double_a.encode().to_le_bytes()),
                v.a_double_w,
                "vector {i}: 2 * a",
            );
        }
    }

    #[rstest]
    fn scalar_mul_matches_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(
            !suite.vectors.scalar_mul.is_empty(),
            "scalar_mul vectors empty"
        );

        for (i, v) in suite.vectors.scalar_mul.iter().enumerate() {
            let base_w = decode_fp5_bytes(&v.base_w);
            let base = Point::decode(base_w).unwrap_or_else(|| panic!("vector {i}: decode base"));
            let s = decode_scalar(&v.scalar_le);

            let out = base * s;
            assert_eq!(
                bytes_to_hex(&out.encode().to_le_bytes()),
                v.out_w,
                "vector {i}: s * base",
            );
        }
    }

    #[rstest]
    fn scalar_ops_match_go_reference_vectors() {
        let suite: VectorsFile = serde_json::from_str(VECTORS_JSON).expect("parse vectors");
        assert!(
            !suite.vectors.scalar_ops.is_empty(),
            "scalar_ops vectors empty"
        );

        for (i, v) in suite.vectors.scalar_ops.iter().enumerate() {
            let a = decode_scalar(&v.a);
            let b = decode_scalar(&v.b);

            assert_eq!(
                bytes_to_hex(&(a + b).to_le_bytes()),
                v.add,
                "vector {i}: a + b",
            );
            assert_eq!(
                bytes_to_hex(&(a - b).to_le_bytes()),
                v.sub,
                "vector {i}: a - b",
            );
            assert_eq!(
                bytes_to_hex(&(a * b).to_le_bytes()),
                v.mul,
                "vector {i}: a * b",
            );
            assert_eq!(bytes_to_hex(&(-a).to_le_bytes()), v.neg_a, "vector {i}: -a",);
            assert_eq!(a.is_zero(), v.a_is_zero, "vector {i}: is_zero");
        }
    }
}
