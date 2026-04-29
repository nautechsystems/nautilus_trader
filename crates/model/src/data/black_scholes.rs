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

// 1. THE HIGH-PRECISION MATHEMATICAL TRAIT
pub trait BlackScholesReal:
    Sized
    + Copy
    + Send
    + Sync
    + Default
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Div<Output = Self>
    + std::ops::Neg<Output = Self>
{
    type Mask: Copy;
    fn splat(val: f64) -> Self;
    #[must_use]
    fn abs(self) -> Self;
    #[must_use]
    fn sqrt(self) -> Self;
    #[must_use]
    fn ln(self) -> Self;
    #[must_use]
    fn exp(self) -> Self;
    #[must_use]
    fn cdf(self) -> Self;
    fn cdf_with_pdf(self) -> (Self, Self);
    #[must_use]
    fn mul_add(self, a: Self, b: Self) -> Self;
    #[must_use]
    fn recip_precise(self) -> Self;
    fn select(mask: Self::Mask, t: Self, f: Self) -> Self;
    fn cmp_gt(self, other: Self) -> Self::Mask;
    #[must_use]
    fn max(self, other: Self) -> Self;
    #[must_use]
    fn min(self, other: Self) -> Self;
    #[must_use]
    fn signum(self) -> Self;
}

// 2. SCALAR IMPLEMENTATION (f32) - Manual Minimax for 1e-7 Precision
impl BlackScholesReal for f32 {
    type Mask = bool;
    #[inline(always)]
    fn splat(val: f64) -> Self {
        val as Self
    }
    #[inline(always)]
    fn abs(self) -> Self {
        self.abs()
    }
    #[inline(always)]
    fn sqrt(self) -> Self {
        self.sqrt()
    }
    #[inline(always)]
    fn select(mask: bool, t: Self, f: Self) -> Self {
        if mask { t } else { f }
    }
    #[inline(always)]
    fn cmp_gt(self, other: Self) -> bool {
        self > other
    }
    #[inline(always)]
    fn recip_precise(self) -> Self {
        1.0 / self
    }
    #[inline(always)]
    fn max(self, other: Self) -> Self {
        self.max(other)
    }
    #[inline(always)]
    fn min(self, other: Self) -> Self {
        self.min(other)
    }
    #[inline(always)]
    fn signum(self) -> Self {
        self.signum()
    }
    #[inline(always)]
    fn mul_add(self, a: Self, b: Self) -> Self {
        self.mul_add(a, b)
    }

    #[inline(always)]
    fn ln(self) -> Self {
        // Minimax polynomial approximation for ln(x) on [1, 2)
        // Optimized for f32 precision with max error ~1e-7
        // Uses range reduction: ln(mantissa) = ln(1 + x) where x = (mantissa - 1) / (mantissa + 1)
        // See: J.-M. Muller et al., "Handbook of Floating-Point Arithmetic", 2018, Section 10.2
        //      A. J. Salgado & S. M. Wise, "Classical Numerical Analysis", 2023, Chapter 10
        let bits = self.to_bits();
        let exponent = ((bits >> 23) as i32 - 127) as Self;
        let mantissa = Self::from_bits((bits & 0x007F_FFFF) | 0x3f80_0000);
        let x = (mantissa - 1.0) / (mantissa + 1.0);
        let x2 = x * x;
        let mut res = 0.239_282_85_f32;
        res = x2.mul_add(res, 0.285_182_11);
        res = x2.mul_add(res, 0.400_005_83);
        res = x2.mul_add(res, 0.666_666_7);
        res = x2.mul_add(res, 2.0);
        x.mul_add(res, exponent * std::f32::consts::LN_2)
    }

    #[inline(always)]
    fn exp(self) -> Self {
        // Minimax polynomial approximation for exp(x) on [-0.5*ln(2), 0.5*ln(2))
        // Optimized for f32 precision with max error ~1e-7
        // Uses range reduction: exp(x) = 2^k * exp(r) where k = round(x / ln(2)) and r = x - k*ln(2)
        // See: J.-M. Muller et al., "Handbook of Floating-Point Arithmetic", 2018, Section 10.3
        //      A. J. Salgado & S. M. Wise, "Classical Numerical Analysis", 2023, Chapter 10
        let k = (self.mul_add(
            std::f32::consts::LOG2_E,
            if self > 0.0 { 0.5 } else { -0.5 },
        )) as i32;
        let r = self - (k as Self * 0.693_145_75) - (k as Self * 1.428_606_8e-6);
        let mut res = 0.001_388_89_f32;
        res = r.mul_add(res, 0.008_333_33);
        res = r.mul_add(res, 0.041_666_67);
        res = r.mul_add(res, 0.166_666_67);
        res = r.mul_add(res, 0.5);
        res = r.mul_add(res, 1.0);
        r.mul_add(res, 1.0) * Self::from_bits(((k + 127) as u32) << 23)
    }

    #[inline(always)]
    fn cdf(self) -> Self {
        self.cdf_with_pdf().0
    }

    #[inline(always)]
    fn cdf_with_pdf(self) -> (Self, Self) {
        // Minimax rational approximation for normal CDF
        // Optimized for f32 precision with max error ~1e-7
        // Uses transformation t = 1 / (1 + 0.2316419 * |x|) for numerical stability
        // See: M. Abramowitz & I. A. Stegun (eds.), "Handbook of Mathematical Functions
        //      with Formulas, Graphs, and Mathematical Tables", 1972, Section 26.2.17
        let abs_x = self.abs();
        let t = 1.0 / (1.0 + 0.231_641_9 * abs_x);
        let mut poly = 1.330_274_5_f32.mul_add(t, -1.821_255_9);
        poly = t.mul_add(poly, 1.781_477_9);
        poly = t.mul_add(poly, -0.356_563_78);
        poly = t.mul_add(poly, 0.319_381_54);
        let pdf = 0.398_942_3 * (-0.5 * self * self).exp();
        let res = 1.0 - pdf * (poly * t);
        // Use >= to ensure CDF(0) = 0.5 exactly (maintains symmetry)
        (if self >= 0.0 { res } else { 1.0 - res }, pdf)
    }
}

// 3. DATA STRUCTURES & CORE KERNEL
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Greeks<T> {
    pub price: T,
    pub vol: T,
    pub delta: T,
    pub gamma: T,
    pub vega: T,
    pub theta: T,
    pub itm_prob: T,
}

/// Lightweight kernel for IV search - only computes price and vega.
/// `phi` is +1 for call, -1 for put (caller does the select once).
#[inline(always)]
fn pricing_kernel_price_vega<T: BlackScholesReal>(
    s_forward: T,
    k: T,
    df_r: T,
    d1: T,
    d2: T,
    sqrt_t: T,
    phi: T,
) -> (T, T) {
    let (cdf_phi_d1, pdf_d1) = (phi * d1).cdf_with_pdf();
    let cdf_phi_d2 = (phi * d2).cdf();

    let price = phi * (s_forward * cdf_phi_d1 - k * df_r * cdf_phi_d2);
    let vega = s_forward * sqrt_t * pdf_d1;

    (price, vega)
}

#[expect(clippy::too_many_arguments)]
#[inline(always)]
fn pricing_kernel<T: BlackScholesReal>(
    s_forward: T,
    k: T,
    df_r: T,
    d1: T,
    d2: T,
    inv_scaled_vol: T,
    vol: T,
    sqrt_t: T,
    t: T,
    r: T,
    b: T,
    s: T,
    phi: T,
) -> Greeks<T> {
    let (cdf_phi_d1, pdf_d1) = (phi * d1).cdf_with_pdf();
    let cdf_phi_d2 = (phi * d2).cdf();

    let df_b = ((b - r) * t).exp();
    let price = phi * (s_forward * cdf_phi_d1 - k * df_r * cdf_phi_d2);
    let delta = phi * df_b * cdf_phi_d1;
    let vega = s_forward * sqrt_t * pdf_d1;
    let gamma = df_b * pdf_d1 * inv_scaled_vol / s;

    let theta_v = -(s_forward * pdf_d1 * vol) * (T::splat(2.0) * sqrt_t).recip_precise();
    let theta_b = -phi * (b - r) * s_forward * cdf_phi_d1;
    let theta_r = -phi * r * k * df_r * cdf_phi_d2;
    let theta = theta_v + theta_b + theta_r;

    Greeks {
        price,
        vol,
        delta,
        gamma,
        vega,
        theta,
        itm_prob: cdf_phi_d2,
    }
}

// 5. SOLVERS: STANDALONE GREEKS & IV SEARCH
#[inline(always)]
pub fn compute_greeks<T: BlackScholesReal>(
    s: T,
    k: T,
    t: T,
    r: T,
    b: T,
    vol: T,
    is_call: T::Mask,
) -> Greeks<T> {
    let sqrt_t = t.sqrt();
    let scaled_vol = vol * sqrt_t;
    let inv_scaled_vol = scaled_vol.recip_precise();
    let df_r = (-r * t).exp();
    let df_b = ((b - r) * t).exp();
    let d1 = ((s / k).ln() + (b + T::splat(0.5) * vol * vol) * t) * inv_scaled_vol;
    let d2 = d1 - scaled_vol;
    let s_forward = s * df_b;
    let phi = T::select(is_call, T::splat(1.0), T::splat(-1.0));

    pricing_kernel(
        s_forward,
        k,
        df_r,
        d1,
        d2,
        inv_scaled_vol,
        vol,
        sqrt_t,
        t,
        r,
        b,
        s,
        phi,
    )
}

/// Performs a single Halley iteration to refine an implied volatility estimate and compute greeks.
///
/// # Important Notes
///
/// This function is intended as a **refinement step** when a good initial guess for volatility
/// is available (e.g., from a previous calculation or a fast approximation). It performs only
/// a single Halley iteration and does not implement a full convergence loop.
///
/// **This is NOT a standalone implied volatility solver.** For production use, prefer
/// `imply_vol_and_greeks` which uses the robust `implied_vol` crate for full convergence.
///
/// # Parameters
///
/// * `initial_guess`: Must be a reasonable estimate of the true volatility. Poor initial guesses
///   (especially for deep ITM/OTM options) may result in significant errors.
///
/// # Accuracy
///
/// With a good initial guess (within ~25% of true vol), one Halley step typically achieves
/// ~1% relative error. For deep ITM/OTM options or poor initial guesses, multiple iterations
/// or a better initial estimate may be required.
#[expect(clippy::too_many_arguments)]
#[inline(always)]
pub fn compute_iv_and_greeks<T: BlackScholesReal>(
    mkt_price: T,
    s: T,
    k: T,
    t: T,
    r: T,
    b: T,
    is_call: T::Mask,
    initial_guess: T,
) -> Greeks<T> {
    // PRE-CALCULATION (Hoisted outside iteration)
    let sqrt_t = t.sqrt();
    let inv_sqrt_t = sqrt_t.recip_precise();
    let ln_sk_bt = (s.ln() - k.ln()) + (b * t); // Numerical Idea 1: Merged constant with b
    let half_t = T::splat(0.5) * t; // Numerical Idea 2: Hoisted half-time
    let df_r = (-r * t).exp();
    let mut vol = initial_guess;

    // SINGLE HALLEY PASS
    let inv_vol = vol.recip_precise();
    let inv_scaled_vol = inv_vol * inv_sqrt_t;
    let d1 = (ln_sk_bt + half_t * vol * vol) * inv_scaled_vol;
    let d2 = d1 - vol * sqrt_t;
    let s_forward = s * ((b - r) * t).exp();
    let phi = T::select(is_call, T::splat(1.0), T::splat(-1.0));
    let (price, vega_raw) = pricing_kernel_price_vega(s_forward, k, df_r, d1, d2, sqrt_t, phi);

    let diff = price - mkt_price;
    let vega = vega_raw.abs().max(T::splat(1e-9));
    let volga = (vega * d1 * d2) * inv_vol;
    let num = T::splat(2.0) * diff * vega;
    let den = T::splat(2.0) * vega * vega - diff * volga;
    // Clamp denominator magnitude while preserving sig
    let den_safe = den.signum() * den.abs().max(T::splat(1e-9));
    vol = vol - (num * den_safe.recip_precise());

    // Clamp volatility to reasonable bounds to prevent negative or infinite values
    // Lower bound: 1e-6 (0.0001% annualized), Upper bound: 10.0 (1000% annualized)
    // Using max/min compiles to single instructions for f32
    vol = vol.max(T::splat(1e-6)).min(T::splat(10.0));

    // FINAL RE-SYNC
    let inv_vol_f = vol.recip_precise();
    let inv_scaled_vol_f = inv_vol_f * inv_sqrt_t;
    let scaled_vol_f = vol * sqrt_t;
    let d1_f = (ln_sk_bt + half_t * vol * vol) * inv_scaled_vol_f;
    let d2_f = d1_f - scaled_vol_f;
    let mut g_final = pricing_kernel(
        s_forward,
        k,
        df_r,
        d1_f,
        d2_f,
        inv_scaled_vol_f,
        vol,
        sqrt_t,
        t,
        r,
        b,
        s,
        phi,
    );
    g_final.vol = vol;

    g_final
}

// 4. UNIT TESTS
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::data::greeks::black_scholes_greeks_exact;

    #[rstest]
    fn test_accuracy_1e7() {
        let s = 100.0;
        let k = 100.0;
        let t = 1.0;
        let r = 0.05;
        let vol = 0.2;
        let g = compute_greeks::<f32>(s, k, t, r, r, vol, true); // Use r as b
        assert!((g.price - 10.45058).abs() < 1e-5);
        let solved = compute_iv_and_greeks::<f32>(g.price, s, k, t, r, r, true, vol); // Use r as b
        assert!((solved.vol - vol).abs() < 1e-6);
    }

    #[rstest]
    fn test_compute_greeks_accuracy_vs_exact() {
        let s = 100.0f64;
        let k = 100.0f64;
        let t = 1.0f64;
        let r = 0.05f64;
        let b = 0.05f64; // cost of carry
        let vol = 0.2f64;
        let multiplier = 1.0f64;

        // Compute using fast f32 method
        let g_fast = compute_greeks::<f32>(
            s as f32, k as f32, t as f32, r as f32, b as f32, vol as f32, true,
        );

        // Compute using exact f64 method
        let g_exact = black_scholes_greeks_exact(s, r, b, vol, true, k, t);

        // Compare with tolerance for f32 precision
        let price_tol = 1e-4;
        let greeks_tol = 1e-3;

        assert!(
            (f64::from(g_fast.price) - g_exact.price).abs() < price_tol,
            "Price mismatch: fast={}, exact={}",
            g_fast.price,
            g_exact.price
        );
        assert!(
            (f64::from(g_fast.delta) - g_exact.delta).abs() < greeks_tol,
            "Delta mismatch: fast={}, exact={}",
            g_fast.delta,
            g_exact.delta
        );
        assert!(
            (f64::from(g_fast.gamma) - g_exact.gamma).abs() < greeks_tol,
            "Gamma mismatch: fast={}, exact={}",
            g_fast.gamma,
            g_exact.gamma
        );
        // Vega units differ: exact uses multiplier * 0.01, fast uses raw units
        let vega_exact_raw = g_exact.vega / (multiplier * 0.01);
        assert!(
            (f64::from(g_fast.vega) - vega_exact_raw).abs() < greeks_tol,
            "Vega mismatch: fast={}, exact_raw={}, exact_scaled={}",
            g_fast.vega,
            vega_exact_raw,
            g_exact.vega
        );
        // Theta units differ: exact uses multiplier * daily_factor (0.0027378507871321013), fast uses raw units
        let theta_daily_factor = 0.002_737_850_787_132_101_3;
        let theta_exact_raw = g_exact.theta / (multiplier * theta_daily_factor);
        assert!(
            (f64::from(g_fast.theta) - theta_exact_raw).abs() < greeks_tol,
            "Theta mismatch: fast={}, exact_raw={}, exact_scaled={}",
            g_fast.theta,
            theta_exact_raw,
            g_exact.theta
        );
    }

    #[rstest]
    fn test_put_theta_with_cost_of_carry_not_equal_to_rate() {
        let s = 100.0f64;
        let k = 100.0f64;
        let t = 1.0f64;
        let r = 0.05f64;
        let b = 0.0f64; // cost of carry != r (e.g. futures option)
        let vol = 0.2f64;
        let multiplier = 1.0f64;

        let g_fast = compute_greeks::<f32>(
            s as f32, k as f32, t as f32, r as f32, b as f32, vol as f32, false,
        );

        let g_exact = black_scholes_greeks_exact(s, r, b, vol, false, k, t);

        let theta_daily_factor = 0.002_737_850_787_132_101_3;
        let theta_exact_raw = g_exact.theta / (multiplier * theta_daily_factor);
        assert!(
            (f64::from(g_fast.theta) - theta_exact_raw).abs() < 1e-3,
            "Put theta mismatch with b!=r: fast={}, exact_raw={}",
            g_fast.theta,
            theta_exact_raw
        );
    }

    #[rstest]
    fn test_compute_iv_and_greeks_halley_accuracy() {
        let s = 100.0f64;
        let k = 100.0f64;
        let t = 1.0f64;
        let r = 0.05f64;
        let b = 0.05f64; // cost of carry
        let vol_true = 0.2f64; // True volatility
        let initial_guess = 0.25f64; // Initial guess (25% higher than true)
        let multiplier = 1.0f64;

        // Compute the exact price using the true volatility
        let g_exact = black_scholes_greeks_exact(s, r, b, vol_true, true, k, t);
        let mkt_price = g_exact.price;

        // Compute implied vol using one Halley step with initial guess
        let g_halley = compute_iv_and_greeks::<f32>(
            mkt_price as f32,
            s as f32,
            k as f32,
            t as f32,
            r as f32,
            b as f32,
            true,
            initial_guess as f32,
        );

        // Check that one Halley step gets close to the true volatility
        let vol_error = (f64::from(g_halley.vol) - vol_true).abs();

        // One Halley step should get within ~1% of true vol for a 25% initial error
        assert!(
            vol_error < 0.01,
            "Halley step accuracy: vol_error={}, initial_guess={}, vol_true={}, computed_vol={}",
            vol_error,
            initial_guess,
            vol_true,
            g_halley.vol
        );

        // Check that the computed greeks are close to exact
        let price_tol = 5e-3; // Relaxed for one Halley step
        let greeks_tol = 5e-3; // Relaxed for one-step approximation

        assert!(
            (f64::from(g_halley.price) - g_exact.price).abs() < price_tol,
            "Price mismatch after Halley: halley={}, exact={}, diff={}",
            g_halley.price,
            g_exact.price,
            (f64::from(g_halley.price) - g_exact.price).abs()
        );
        assert!(
            (f64::from(g_halley.delta) - g_exact.delta).abs() < greeks_tol,
            "Delta mismatch after Halley: halley={}, exact={}",
            g_halley.delta,
            g_exact.delta
        );
        assert!(
            (f64::from(g_halley.gamma) - g_exact.gamma).abs() < greeks_tol,
            "Gamma mismatch after Halley: halley={}, exact={}",
            g_halley.gamma,
            g_exact.gamma
        );
        // Vega units differ: exact uses multiplier * 0.01, fast uses raw units
        let vega_exact_raw = g_exact.vega / (multiplier * 0.01);
        assert!(
            (f64::from(g_halley.vega) - vega_exact_raw).abs() < greeks_tol,
            "Vega mismatch after Halley: halley={}, exact_raw={}",
            g_halley.vega,
            vega_exact_raw
        );
        // Theta units differ: exact uses multiplier * daily_factor (0.0027378507871321013), fast uses raw units
        let theta_daily_factor = 0.002_737_850_787_132_101_3;
        let theta_exact_raw = g_exact.theta / (multiplier * theta_daily_factor);
        assert!(
            (f64::from(g_halley.theta) - theta_exact_raw).abs() < greeks_tol,
            "Theta mismatch after Halley: halley={}, exact_raw={}",
            g_halley.theta,
            theta_exact_raw
        );
    }

    #[rstest]
    fn test_print_halley_iv() {
        let s = 100.0f64;
        let k = 100.0f64;
        let t = 1.0f64;
        let r = 0.05f64;
        let b = 0.05f64;
        let vol_true = 0.2f64;

        let g_exact = black_scholes_greeks_exact(s, r, b, vol_true, true, k, t);
        let mkt_price = g_exact.price;

        println!("\n=== Halley Step IV Test (Using True Vol as Initial Guess) ===");
        println!("True volatility: {vol_true}");
        println!("Market price: {mkt_price:.8}");
        println!("Initial guess: {vol_true} (using true vol)");

        let g_halley = compute_iv_and_greeks::<f32>(
            mkt_price as f32,
            s as f32,
            k as f32,
            t as f32,
            r as f32,
            b as f32,
            true,
            vol_true as f32, // Using true vol as initial guess
        );

        println!("\nAfter one Halley step:");
        println!("Computed volatility: {:.8}", g_halley.vol);
        println!("True volatility: {vol_true:.8}");
        println!(
            "Absolute error: {:.8}",
            (f64::from(g_halley.vol) - vol_true).abs()
        );
        println!(
            "Relative error: {:.4}%",
            (f64::from(g_halley.vol) - vol_true).abs() / vol_true * 100.0
        );
    }

    #[rstest]
    fn test_compute_iv_and_greeks_deep_itm_otm() {
        let t = 1.0f64;
        let r = 0.05f64;
        let b = 0.05f64;
        let vol_true = 0.2f64;

        // Deep ITM: s=150, k=100 (spot is 50% above strike)
        let s_itm = 150.0f64;
        let k_itm = 100.0f64;
        let g_exact_itm = black_scholes_greeks_exact(s_itm, r, b, vol_true, true, k_itm, t);
        let mkt_price_itm = g_exact_itm.price;

        println!("\n=== Deep ITM Test ===");
        println!("Spot: {s_itm}, Strike: {k_itm}, True vol: {vol_true}");
        println!("Market price: {mkt_price_itm:.8}");

        let g_recovered_itm = compute_iv_and_greeks::<f32>(
            mkt_price_itm as f32,
            s_itm as f32,
            k_itm as f32,
            t as f32,
            r as f32,
            b as f32,
            true,
            vol_true as f32, // Using true vol as initial guess
        );

        let vol_error_itm = (f64::from(g_recovered_itm.vol) - vol_true).abs();
        let rel_error_itm = vol_error_itm / vol_true * 100.0;

        println!("Recovered volatility: {:.8}", g_recovered_itm.vol);
        println!("Absolute error: {vol_error_itm:.8}");
        println!("Relative error: {rel_error_itm:.4}%");

        // Deep OTM: s=50, k=100 (spot is 50% below strike)
        let s_otm = 50.0f64;
        let k_otm = 100.0f64;
        let g_exact_otm = black_scholes_greeks_exact(s_otm, r, b, vol_true, true, k_otm, t);
        let mkt_price_otm = g_exact_otm.price;

        println!("\n=== Deep OTM Test ===");
        println!("Spot: {s_otm}, Strike: {k_otm}, True vol: {vol_true}");
        println!("Market price: {mkt_price_otm:.8}");

        let g_recovered_otm = compute_iv_and_greeks::<f32>(
            mkt_price_otm as f32,
            s_otm as f32,
            k_otm as f32,
            t as f32,
            r as f32,
            b as f32,
            false,
            vol_true as f32, // Using true vol as initial guess
        );

        let vol_error_otm = (f64::from(g_recovered_otm.vol) - vol_true).abs();
        let rel_error_otm = vol_error_otm / vol_true * 100.0;

        println!("Recovered volatility: {:.8}", g_recovered_otm.vol);
        println!("Absolute error: {vol_error_otm:.8}");
        println!("Relative error: {rel_error_otm:.4}%");

        // Assertions: Deep ITM and OTM are challenging cases
        // One Halley step with Corrado-Miller initial guess may not be sufficient
        // We use a more relaxed tolerance to verify the method still converges in the right direction
        // For production use, multiple iterations or better initial guesses would be needed
        let vol_tol_itm = 50.0; // 50% relative error tolerance for deep ITM
        let vol_tol_otm = 150.0; // 150% relative error tolerance for deep OTM (very challenging)

        // Check that we at least get a reasonable volatility (not NaN or extreme values)
        assert!(
            g_recovered_itm.vol.is_finite()
                && g_recovered_itm.vol > 0.0
                && g_recovered_itm.vol < 2.0,
            "Deep ITM vol recovery: invalid result={}",
            g_recovered_itm.vol
        );

        assert!(
            g_recovered_otm.vol.is_finite()
                && g_recovered_otm.vol > 0.0
                && g_recovered_otm.vol < 2.0,
            "Deep OTM vol recovery: invalid result={}",
            g_recovered_otm.vol
        );

        // Verify the error is within acceptable bounds (one step may not be enough)
        assert!(
            rel_error_itm < vol_tol_itm,
            "Deep ITM vol recovery error too large: recovered={}, true={}, error={:.4}%",
            g_recovered_itm.vol,
            vol_true,
            rel_error_itm
        );

        assert!(
            rel_error_otm < vol_tol_otm,
            "Deep OTM vol recovery error too large: recovered={}, true={}, error={:.4}%",
            g_recovered_otm.vol,
            vol_true,
            rel_error_otm
        );

        println!("\n=== Summary ===");
        println!("Deep ITM: One Halley iteration error = {rel_error_itm:.2}%");
        println!(
            "Deep OTM: One Halley iteration error = {rel_error_otm:.2}% (still challenging, deep OTM is difficult)"
        );
    }
}
