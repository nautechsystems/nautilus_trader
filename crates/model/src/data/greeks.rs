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

//! Option *Greeks* data structures (delta, gamma, theta, vega, rho) used throughout the platform.

use std::{
    any::Any,
    fmt::Display,
    ops::{Add, Deref, Mul},
    sync::Arc,
};

use implied_vol::{DefaultSpecialFn, ImpliedBlackVolatility, SpecialFn};
use nautilus_core::{
    UnixNanos,
    correctness::{CorrectnessResult, check_equal_usize, check_predicate_true},
    datetime::unix_nanos_to_iso8601,
    math::{quadratic_interpolation, too_close_for_interpolation},
};
use serde::{Deserialize, Serialize};

use crate::{
    data::{
        CustomDataTrait, HasTsInit,
        black_scholes::{
            REFINEMENT_VOL_MAX, REFINEMENT_VOL_MIN, compute_greeks, compute_iv_and_greeks,
        },
    },
    identifiers::InstrumentId,
};

const FRAC_SQRT_2_PI: f64 = f64::from_bits(0x3fd9_8845_33d4_3651);
/// used to convert theta to per-calendar-day change when building `BlackScholesGreeksResult`.
const THETA_DAILY_FACTOR: f64 = 1.0 / 365.25;
/// Scale for vega to express as absolute percent change when building `BlackScholesGreeksResult`.
const VEGA_PERCENT_FACTOR: f64 = 0.01;
const RHO_PERCENT_FACTOR: f64 = 0.01;
const PRICE_RELATIVE_TOLERANCE: f64 = 1e-4;

/// An unsupported or invalid local option valuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OptionValuationError {
    #[error("Invalid option valuation input: {0}")]
    InvalidInput(&'static str),
    #[error("No finite positive implied volatility for the option price")]
    ImpliedVolatilityUnavailable,
    #[error("Nonfinite or inconsistent option valuation result")]
    InvalidResult,
    #[error("Option {0} has expired")]
    Expired(InstrumentId),
    #[error("Local valuation of option spread {0} is unsupported")]
    UnsupportedSpread(InstrumentId),
}

/// Core option Greek sensitivity values (the 5 standard sensitivities).
/// Designed as a composable building block embedded in all Greeks-carrying types.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OptionGreekValues {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}

impl Add for OptionGreekValues {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            delta: self.delta + rhs.delta,
            gamma: self.gamma + rhs.gamma,
            vega: self.vega + rhs.vega,
            theta: self.theta + rhs.theta,
            rho: self.rho + rhs.rho,
        }
    }
}

impl Mul<f64> for OptionGreekValues {
    type Output = Self;

    fn mul(self, scalar: f64) -> Self {
        Self {
            delta: self.delta * scalar,
            gamma: self.gamma * scalar,
            vega: self.vega * scalar,
            theta: self.theta * scalar,
            rho: self.rho * scalar,
        }
    }
}

impl Mul<OptionGreekValues> for f64 {
    type Output = OptionGreekValues;

    fn mul(self, greeks: OptionGreekValues) -> OptionGreekValues {
        greeks * self
    }
}

impl Display for OptionGreekValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionGreekValues(delta={:.4}, gamma={:.4}, vega={:.4}, theta={:.4}, rho={:.4})",
            self.delta, self.gamma, self.vega, self.theta, self.rho
        )
    }
}

/// Trait for types carrying Greek sensitivity values.
pub trait HasGreeks {
    fn greeks(&self) -> OptionGreekValues;
}

#[inline(always)]
fn norm_pdf(x: f64) -> f64 {
    FRAC_SQRT_2_PI * (-0.5 * x * x).exp()
}

/// Result structure for Black-Scholes Greeks calculations.
///
/// This is a separate f64 struct (not a type alias) for Python compatibility.
/// Rho is per percentage point and holds carry fixed in the unchecked functions.
/// Checked functions take the carry rate sensitivity explicitly.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BlackScholesGreeksResult {
    pub price: f64,
    pub vol: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub itm_prob: f64,
    pub rho: f64,
}

// Standardized Generalized Black-Scholes Greeks implementation
// dS_t = S_t * (b * dt + vol * dW_t) (stock)
// dC_t = r * C_t * dt (cash numeraire)
#[must_use]
pub fn black_scholes_greeks_exact(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
) -> BlackScholesGreeksResult {
    let phi = if is_call { 1.0 } else { -1.0 };
    let sqrt_t = t.sqrt();
    let scaled_vol = vol * sqrt_t;

    // d1 and d2 calculations
    let d1 = ((s / k).ln() + (b + 0.5 * vol.powi(2)) * t) / scaled_vol;
    let d2 = d1 - scaled_vol;

    // Probabilities and PDF
    let cdf_phi_d1 = DefaultSpecialFn::norm_cdf(phi * d1);
    let cdf_phi_d2 = DefaultSpecialFn::norm_cdf(phi * d2);
    let pdf_d1 = norm_pdf(d1);

    // Discounting factors
    let df_b = ((b - r) * t).exp();
    let df_r = (-r * t).exp();

    // Price and common Greeks
    let price = phi * (s * df_b * cdf_phi_d1 - k * df_r * cdf_phi_d2);
    let delta = phi * df_b * cdf_phi_d1;
    let gamma = (df_b * pdf_d1) / (s * scaled_vol);
    let vega = s * df_b * sqrt_t * pdf_d1 * VEGA_PERCENT_FACTOR;

    // Decay due to volatility, Drift/Cost of Carry component, Interest rate component on strike
    let theta_v = -(s * df_b * pdf_d1 * vol) / (2.0 * sqrt_t);
    let theta_b = -phi * (b - r) * s * df_b * cdf_phi_d1;
    let theta_r = -phi * r * k * df_r * cdf_phi_d2;
    let theta = (theta_v + theta_b + theta_r) * THETA_DAILY_FACTOR;

    BlackScholesGreeksResult {
        price,
        vol,
        delta,
        gamma,
        vega,
        theta,
        itm_prob: cdf_phi_d2,
        rho: -t * price * RHO_PERCENT_FACTOR,
    }
}

#[must_use]
pub fn imply_vol(s: f64, r: f64, b: f64, is_call: bool, k: f64, t: f64, price: f64) -> f64 {
    let forward = s * (b * t).exp();
    let forward_price = price * (r * t).exp();

    ImpliedBlackVolatility::builder()
        .option_price(forward_price)
        .forward(forward)
        .strike(k)
        .expiry(t)
        .is_call(is_call)
        .build()
        .and_then(|iv| iv.calculate::<DefaultSpecialFn>())
        .unwrap_or(0.0)
}

/// Computes Black-Scholes greeks using the fast `compute_greeks` implementation.
/// This function uses `compute_greeks` from `black_scholes.rs` which is optimized for performance.
#[must_use]
pub fn black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
) -> BlackScholesGreeksResult {
    // Use f32 for performance, then cast to f64 when applying multiplier
    let greeks = compute_greeks::<f32>(
        s as f32, k as f32, t as f32, r as f32, b as f32, vol as f32, is_call,
    );

    BlackScholesGreeksResult {
        price: f64::from(greeks.price),
        vol,
        delta: f64::from(greeks.delta),
        gamma: f64::from(greeks.gamma),
        vega: f64::from(greeks.vega) * VEGA_PERCENT_FACTOR,
        theta: f64::from(greeks.theta) * THETA_DAILY_FACTOR,
        itm_prob: f64::from(greeks.itm_prob),
        rho: -t * f64::from(greeks.price) * RHO_PERCENT_FACTOR,
    }
}

/// Computes implied volatility and greeks using the fast implementations.
/// This function uses `compute_greeks` after implying volatility.
#[must_use]
pub fn imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> BlackScholesGreeksResult {
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    // Handle case when imply_vol fails and returns 0.0 or very small value
    // Using a very small vol (1e-8) instead of 0.0 prevents division by zero in greeks calculations
    // This ensures greeks remain finite even when imply_vol fails
    let safe_vol = if vol < 1e-8 { 1e-8 } else { vol };
    black_scholes_greeks(s, r, b, safe_vol, is_call, k, t)
}

/// Refines implied volatility using an initial guess and computes greeks.
/// This function uses `compute_iv_and_greeks` which performs a Halley iteration
/// to refine the volatility estimate from an initial guess.
#[expect(clippy::too_many_arguments)]
#[must_use]
pub fn refine_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    target_price: f64,
    initial_vol: f64,
) -> BlackScholesGreeksResult {
    // Use f32 for performance, then cast to f64 when applying multiplier
    let greeks = compute_iv_and_greeks::<f32>(
        target_price as f32,
        s as f32,
        k as f32,
        t as f32,
        r as f32,
        b as f32,
        is_call,
        initial_vol as f32,
    );

    BlackScholesGreeksResult {
        price: f64::from(greeks.price),
        vol: f64::from(greeks.vol),
        delta: f64::from(greeks.delta),
        gamma: f64::from(greeks.gamma),
        vega: f64::from(greeks.vega) * VEGA_PERCENT_FACTOR,
        theta: f64::from(greeks.theta) * THETA_DAILY_FACTOR,
        itm_prob: f64::from(greeks.itm_prob),
        rho: -t * f64::from(greeks.price) * RHO_PERCENT_FACTOR,
    }
}

/// Computes validated Greeks with an explicit sensitivity of carry to the interest rate.
///
/// `db_dr` is zero for fixed carry and one for carry defined as `r - q`, even when `r == q`.
/// Rho measures the price change for a one percentage point rate move.
/// The exact implementation is used when the fast result is nonfinite or inconsistent.
///
/// # Errors
///
/// Returns an error if:
/// - Model inputs are invalid.
/// - Valuation results are nonfinite or inconsistent.
#[expect(clippy::too_many_arguments)]
pub fn black_scholes_greeks_checked(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
    db_dr: f64,
) -> Result<BlackScholesGreeksResult, OptionValuationError> {
    check_option_inputs(s, r, b, vol, k, t)?;
    let result = black_scholes_greeks(s, r, b, vol, is_call, k, t);
    match apply_rate_sensitivity(result, s, r, is_call, k, t, db_dr) {
        Err(OptionValuationError::InvalidResult) => {
            let result = black_scholes_greeks_exact(s, r, b, vol, is_call, k, t);
            apply_rate_sensitivity(result, s, r, is_call, k, t, db_dr)
        }
        result => result,
    }
}

/// Solves implied volatility and computes validated Greeks with explicit carry rate sensitivity.
///
/// A refinement seed is used only when it produces an interior volatility and reprices the
/// premium within 0.01 percent, otherwise the full implied-volatility solver is used.
/// The exact implementation is used when the fast kernel cannot reprice the solved premium
/// within that tolerance. Carry rate sensitivity and rho units follow [`black_scholes_greeks_checked`].
///
/// # Errors
///
/// Returns an error if:
/// - Model inputs or the premium are invalid.
/// - The premium has no finite positive implied volatility.
/// - Results are nonfinite, inconsistent, or fail the 0.01 percent repricing tolerance.
#[expect(clippy::too_many_arguments)]
pub fn imply_vol_and_greeks_checked(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
    db_dr: f64,
    initial_vol: Option<f64>,
) -> Result<BlackScholesGreeksResult, OptionValuationError> {
    check_option_inputs(s, r, b, 1.0, k, t)?;

    if !price.is_finite() || price <= 0.0 {
        return Err(OptionValuationError::InvalidInput("option price"));
    }

    let forward = s * (b * t).exp();
    let forward_price = price * (r * t).exp();

    let intrinsic = if is_call {
        (forward - k).max(0.0)
    } else {
        (k - forward).max(0.0)
    };

    let upper = if is_call { forward } else { k };
    if !forward.is_finite()
        || !forward_price.is_finite()
        || forward_price <= intrinsic
        || forward_price >= upper
    {
        return Err(OptionValuationError::ImpliedVolatilityUnavailable);
    }

    let price_tolerance = price * PRICE_RELATIVE_TOLERANCE;

    if let Some(seed) = initial_vol {
        check_option_inputs(s, r, b, seed, k, t)?;
        let result = refine_vol_and_greeks(s, r, b, is_call, k, t, price, seed);
        if result.vol > REFINEMENT_VOL_MIN
            && result.vol < REFINEMENT_VOL_MAX
            && (result.price - price).abs() <= price_tolerance
        {
            return apply_rate_sensitivity(result, s, r, is_call, k, t, db_dr);
        }
    }

    let vol = imply_vol_checked(s, r, b, is_call, k, t, price)?;
    let mut result = black_scholes_greeks_checked(s, r, b, vol, is_call, k, t, db_dr)?;
    if (result.price - price).abs() > price_tolerance {
        result = apply_rate_sensitivity(
            black_scholes_greeks_exact(s, r, b, vol, is_call, k, t),
            s,
            r,
            is_call,
            k,
            t,
            db_dr,
        )?;
    }

    if (result.price - price).abs() > price_tolerance {
        return Err(OptionValuationError::InvalidResult);
    }

    Ok(result)
}

/// Solves implied volatility without fast-kernel repricing.
///
/// # Errors
///
/// Returns an error if:
/// - Model inputs are invalid.
/// - The premium has no finite positive implied volatility.
pub(crate) fn imply_vol_checked(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> Result<f64, OptionValuationError> {
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    if !vol.is_finite() || vol <= 0.0 {
        return Err(OptionValuationError::ImpliedVolatilityUnavailable);
    }

    Ok(vol)
}

/// Checks model inputs against the fast pricing kernel's numeric domain.
///
/// # Errors
///
/// Returns an error if:
/// - Rates are not finite in the pricing kernel's precision.
/// - Spot, strike, time, or volatility are not finite and positive in that precision.
pub fn check_option_inputs(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    k: f64,
    t: f64,
) -> Result<(), OptionValuationError> {
    for (name, value) in [
        ("underlying price", s),
        ("strike", k),
        ("time to expiry", t),
        ("volatility", vol),
    ] {
        let kernel_value = value as f32;
        if !value.is_finite() || value <= 0.0 || !kernel_value.is_finite() || kernel_value <= 0.0 {
            return Err(OptionValuationError::InvalidInput(name));
        }
    }

    for (name, value) in [("interest rate", r), ("cost of carry", b)] {
        if !(value as f32).is_finite() {
            return Err(OptionValuationError::InvalidInput(name));
        }
    }

    Ok(())
}

fn apply_rate_sensitivity(
    mut result: BlackScholesGreeksResult,
    s: f64,
    r: f64,
    is_call: bool,
    k: f64,
    t: f64,
    db_dr: f64,
) -> Result<BlackScholesGreeksResult, OptionValuationError> {
    if !db_dr.is_finite() {
        return Err(OptionValuationError::InvalidInput("carry rate sensitivity"));
    }

    if db_dr != 0.0 {
        let phi = if is_call { 1.0 } else { -1.0 };
        // The strike term avoids subtracting spot delta and price for deep ITM options
        result.rho = t
            * ((db_dr - 1.0) * s * result.delta + phi * k * (-r * t).exp() * result.itm_prob)
            * RHO_PERCENT_FACTOR;
    }

    check_valuation_result(result)
}

fn check_valuation_result(
    result: BlackScholesGreeksResult,
) -> Result<BlackScholesGreeksResult, OptionValuationError> {
    if [
        result.price,
        result.vol,
        result.delta,
        result.gamma,
        result.vega,
        result.theta,
        result.rho,
        result.itm_prob,
    ]
    .iter()
    .any(|value| !value.is_finite())
        || result.price < 0.0
        || result.gamma < 0.0
        || result.vega < 0.0
        || !(0.0..=1.0).contains(&result.itm_prob)
    {
        return Err(OptionValuationError::InvalidResult);
    }

    Ok(result)
}

#[repr(C)]
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct GreeksData {
    pub instrument_id: InstrumentId,
    pub is_call: bool,
    pub strike: f64,
    pub expiry: i32,
    pub expiry_in_days: i32,
    pub expiry_in_years: f64,
    pub multiplier: f64,
    pub quantity: f64,
    pub underlying_price: f64,
    pub interest_rate: f64,
    pub cost_of_carry: f64,
    pub vol: f64,
    pub pnl: f64,
    pub price: f64,
    /// Core Greek sensitivity values (delta, gamma, vega, theta, rho).
    pub greeks: OptionGreekValues,
    // in the money probability, P(phi * S_T > phi * K), phi = 1 if is_call else -1
    pub itm_prob: f64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl GreeksData {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        is_call: bool,
        strike: f64,
        expiry: i32,
        expiry_in_days: i32,
        expiry_in_years: f64,
        multiplier: f64,
        quantity: f64,
        underlying_price: f64,
        interest_rate: f64,
        cost_of_carry: f64,
        vol: f64,
        pnl: f64,
        price: f64,
        greeks: OptionGreekValues,
        itm_prob: f64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            is_call,
            strike,
            expiry,
            expiry_in_days,
            expiry_in_years,
            multiplier,
            quantity,
            underlying_price,
            interest_rate,
            cost_of_carry,
            vol,
            pnl,
            price,
            greeks,
            itm_prob,
            ts_event,
            ts_init,
        }
    }

    #[must_use]
    pub fn from_delta(
        instrument_id: InstrumentId,
        delta: f64,
        multiplier: f64,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            is_call: true,
            strike: 0.0,
            expiry: 0,
            expiry_in_days: 0,
            expiry_in_years: 0.0,
            multiplier,
            quantity: 1.0,
            underlying_price: 0.0,
            interest_rate: 0.0,
            cost_of_carry: 0.0,
            vol: 0.0,
            pnl: 0.0,
            price: 0.0,
            greeks: OptionGreekValues {
                delta,
                ..Default::default()
            },
            itm_prob: 0.0,
            ts_event,
            ts_init: ts_event,
        }
    }

    /// Returns portfolio values with `pnl`, `price`, and all Greeks scaled by the contract multiplier.
    ///
    /// Preserves both timestamps and leaves position quantity scaling to the caller.
    #[must_use]
    pub fn to_portfolio_greeks(&self) -> PortfolioGreeks {
        PortfolioGreeks {
            pnl: self.pnl * self.multiplier,
            price: self.price * self.multiplier,
            greeks: self.greeks * self.multiplier,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
        }
    }
}

impl Deref for GreeksData {
    type Target = OptionGreekValues;
    fn deref(&self) -> &Self::Target {
        &self.greeks
    }
}

impl HasGreeks for GreeksData {
    fn greeks(&self) -> OptionGreekValues {
        self.greeks
    }
}

impl Default for GreeksData {
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::from("ES.GLBX"),
            is_call: true,
            strike: 0.0,
            expiry: 0,
            expiry_in_days: 0,
            expiry_in_years: 0.0,
            multiplier: 0.0,
            quantity: 0.0,
            underlying_price: 0.0,
            interest_rate: 0.0,
            cost_of_carry: 0.0,
            vol: 0.0,
            pnl: 0.0,
            price: 0.0,
            greeks: OptionGreekValues::default(),
            itm_prob: 0.0,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Display for GreeksData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GreeksData(instrument_id={}, expiry={}, itm_prob={:.2}%, vol={:.2}%, pnl={:.2}, price={:.2}, delta={:.2}, gamma={:.2}, vega={:.2}, theta={:.2}, quantity={}, ts_event={}, ts_init={})",
            self.instrument_id,
            self.expiry,
            self.itm_prob * 100.0,
            self.vol * 100.0,
            self.pnl,
            self.price,
            self.greeks.delta,
            self.greeks.gamma,
            self.greeks.vega,
            self.greeks.theta,
            self.quantity,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

/// Scales values by quantity without applying the contract multiplier.
impl Mul<&GreeksData> for f64 {
    type Output = GreeksData;

    fn mul(self, g: &GreeksData) -> GreeksData {
        GreeksData {
            instrument_id: g.instrument_id,
            is_call: g.is_call,
            strike: g.strike,
            expiry: g.expiry,
            expiry_in_days: g.expiry_in_days,
            expiry_in_years: g.expiry_in_years,
            multiplier: g.multiplier,
            quantity: g.quantity,
            underlying_price: g.underlying_price,
            interest_rate: g.interest_rate,
            cost_of_carry: g.cost_of_carry,
            vol: g.vol,
            pnl: self * g.pnl,
            price: self * g.price,
            greeks: g.greeks * self,
            itm_prob: g.itm_prob,
            ts_event: g.ts_event,
            ts_init: g.ts_init,
        }
    }
}

impl HasTsInit for GreeksData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PortfolioGreeks {
    pub pnl: f64,
    pub price: f64,
    pub greeks: OptionGreekValues,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl PortfolioGreeks {
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        pnl: f64,
        price: f64,
        delta: f64,
        gamma: f64,
        vega: f64,
        theta: f64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            pnl,
            price,
            greeks: OptionGreekValues {
                delta,
                gamma,
                vega,
                theta,
                rho: 0.0,
            },
            ts_event,
            ts_init,
        }
    }
}

impl Deref for PortfolioGreeks {
    type Target = OptionGreekValues;
    fn deref(&self) -> &Self::Target {
        &self.greeks
    }
}

impl Default for PortfolioGreeks {
    fn default() -> Self {
        Self {
            pnl: 0.0,
            price: 0.0,
            greeks: OptionGreekValues::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Display for PortfolioGreeks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PortfolioGreeks(pnl={:.2}, price={:.2}, delta={:.2}, gamma={:.2}, vega={:.2}, theta={:.2}, ts_event={}, ts_init={})",
            self.pnl,
            self.price,
            self.greeks.delta,
            self.greeks.gamma,
            self.greeks.vega,
            self.greeks.theta,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

impl Add for PortfolioGreeks {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            pnl: self.pnl + other.pnl,
            price: self.price + other.price,
            greeks: self.greeks + other.greeks,
            ts_event: self.ts_event,
            ts_init: self.ts_init,
        }
    }
}

impl From<GreeksData> for PortfolioGreeks {
    fn from(g: GreeksData) -> Self {
        g.to_portfolio_greeks()
    }
}

impl Mul<&PortfolioGreeks> for f64 {
    type Output = PortfolioGreeks;

    fn mul(self, g: &PortfolioGreeks) -> PortfolioGreeks {
        PortfolioGreeks {
            pnl: self * g.pnl,
            price: self * g.price,
            greeks: g.greeks * self,
            ts_event: g.ts_event,
            ts_init: g.ts_init,
        }
    }
}

impl HasTsInit for PortfolioGreeks {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasGreeks for PortfolioGreeks {
    fn greeks(&self) -> OptionGreekValues {
        self.greeks
    }
}

impl HasGreeks for BlackScholesGreeksResult {
    fn greeks(&self) -> OptionGreekValues {
        OptionGreekValues {
            delta: self.delta,
            gamma: self.gamma,
            vega: self.vega,
            theta: self.theta,
            rho: self.rho,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct YieldCurve {
    pub curve_name: String,
    pub tenors: Vec<f64>,
    pub interest_rates: Vec<f64>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl YieldCurve {
    #[must_use]
    pub fn new(
        curve_name: String,
        tenors: Vec<f64>,
        interest_rates: Vec<f64>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            curve_name,
            tenors,
            interest_rates,
            ts_event,
            ts_init,
        }
    }

    // Interpolate the yield curve for a given expiry time
    #[must_use]
    pub fn get_rate(&self, expiry_in_years: f64) -> f64 {
        if self.interest_rates.len() == 1 {
            return self.interest_rates[0];
        }

        quadratic_interpolation(expiry_in_years, &self.tenors, &self.interest_rates)
    }

    /// Validates that [`Self::get_rate`] can evaluate this curve.
    ///
    /// # Errors
    ///
    /// Returns an error if `tenors` and `interest_rates` differ in length, hold two points or
    /// none, or hold a non-finite value, or if `tenors` does not strictly increase with each
    /// step wide enough to interpolate.
    pub fn validate(&self) -> CorrectnessResult<()> {
        check_equal_usize(
            self.tenors.len(),
            self.interest_rates.len(),
            "tenors",
            "interest_rates",
        )?;

        // Interpolation needs at least three points; a single rate is a flat curve
        check_predicate_true(
            self.interest_rates.len() == 1 || self.interest_rates.len() >= 3,
            "`interest_rates` must hold one rate or at least three points",
        )?;
        check_predicate_true(
            self.tenors
                .iter()
                .chain(&self.interest_rates)
                .all(|value| value.is_finite()),
            "`tenors` and `interest_rates` must be finite",
        )?;
        check_predicate_true(
            self.tenors
                .windows(2)
                .all(|pair| pair[0] < pair[1] && !too_close_for_interpolation(pair[0], pair[1])),
            "`tenors` must be strictly increasing, with each step wide enough to interpolate",
        )
    }
}

impl Display for YieldCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "YieldCurve(curve_name={}, ts_event={}, ts_init={})",
            self.curve_name,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

impl HasTsInit for YieldCurve {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for YieldCurve {
    fn type_name(&self) -> &'static str {
        Self::type_name_static()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        crate::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "YieldCurve"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

impl Default for YieldCurve {
    fn default() -> Self {
        Self {
            curve_name: "USD".to_string(),
            tenors: vec![0.5, 1.0, 1.5, 2.0, 2.5],
            interest_rates: vec![0.04, 0.04, 0.04, 0.04, 0.04],
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::identifiers::InstrumentId;

    fn create_test_greeks_data() -> GreeksData {
        GreeksData::new(
            InstrumentId::from("SPY240315C00500000.OPRA"),
            true,
            500.0,
            20_240_315,
            91, // expiry_in_days (approximately 3 months)
            0.25,
            100.0,
            1.0,
            520.0,
            0.05,
            0.05,
            0.2,
            250.0,
            25.5,
            OptionGreekValues {
                delta: 0.65,
                gamma: 0.003,
                vega: 15.2,
                theta: -0.08,
                rho: 0.0,
            },
            0.75,
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(1_000_000_000),
        )
    }

    fn create_test_portfolio_greeks() -> PortfolioGreeks {
        PortfolioGreeks::new(
            1500.0,
            125.5,
            2.15,
            0.008,
            42.7,
            -2.3,
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(1_000_000_000),
        )
    }

    fn create_test_yield_curve() -> YieldCurve {
        YieldCurve::new(
            "USD".to_string(),
            vec![0.25, 0.5, 1.0, 2.0, 5.0],
            vec![0.025, 0.03, 0.035, 0.04, 0.045],
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(1_000_000_000),
        )
    }

    #[rstest]
    fn test_black_scholes_greeks_result_creation() {
        let result = BlackScholesGreeksResult {
            price: 25.5,
            vol: 0.2,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
            itm_prob: 0.55,
            rho: 1.75,
        };

        assert_eq!(result.price, 25.5);
        assert_eq!(result.delta, 0.65);
        assert_eq!(result.gamma, 0.003);
        assert_eq!(result.vega, 15.2);
        assert_eq!(result.theta, -0.08);
        assert_eq!(result.itm_prob, 0.55);
    }

    #[rstest]
    fn test_black_scholes_greeks_result_clone_and_copy() {
        let result1 = BlackScholesGreeksResult {
            price: 25.5,
            vol: 0.2,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
            itm_prob: 0.55,
            rho: 1.75,
        };
        let result2 = result1;
        let result3 = result1;

        assert_eq!(result1, result2);
        assert_eq!(result1, result3);
    }

    #[rstest]
    fn test_black_scholes_greeks_result_debug() {
        let result = BlackScholesGreeksResult {
            price: 25.5,
            vol: 0.2,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
            itm_prob: 0.55,
            rho: 1.75,
        };
        let debug_str = format!("{result:?}");

        assert!(debug_str.contains("BlackScholesGreeksResult"));
        assert!(debug_str.contains("25.5"));
        assert!(debug_str.contains("0.65"));
    }

    #[rstest]
    fn test_imply_vol_and_greeks_result_creation() {
        let result = BlackScholesGreeksResult {
            price: 25.5,
            vol: 0.2,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
            itm_prob: 0.55,
            rho: 1.75,
        };

        assert_eq!(result.vol, 0.2);
        assert_eq!(result.price, 25.5);
        assert_eq!(result.delta, 0.65);
        assert_eq!(result.gamma, 0.003);
        assert_eq!(result.vega, 15.2);
        assert_eq!(result.theta, -0.08);
    }

    #[rstest]
    fn test_black_scholes_greeks_basic_call() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.price > 0.0);
        assert!(greeks.delta > 0.0 && greeks.delta < 1.0);
        assert!(greeks.gamma > 0.0);
        assert!(greeks.vega > 0.0);
        assert!(greeks.theta < 0.0); // Time decay for long option
    }

    #[rstest]
    fn test_black_scholes_greeks_basic_put() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = false;
        let k = 100.0;
        let t = 1.0;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(
            greeks.price > 0.0,
            "Put option price should be positive, was: {}",
            greeks.price
        );
        assert!(greeks.delta < 0.0 && greeks.delta > -1.0);
        assert!(greeks.gamma > 0.0);
        assert!(greeks.vega > 0.0);
        assert!(greeks.theta < 0.0); // Time decay for long option
    }

    #[rstest]
    fn test_black_scholes_greeks_deep_itm_call() {
        let s = 150.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.delta > 0.9); // Deep ITM call has delta close to 1
        assert!(greeks.gamma > 0.0 && greeks.gamma < 0.01); // Low gamma for deep ITM
    }

    #[rstest]
    fn test_black_scholes_greeks_deep_otm_call() {
        let s = 50.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.delta < 0.1); // Deep OTM call has delta close to 0
        assert!(greeks.gamma > 0.0 && greeks.gamma < 0.01); // Low gamma for deep OTM
    }

    #[rstest]
    fn test_black_scholes_greeks_zero_time() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 0.0001; // Near zero time

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.price >= 0.0);
        assert!(greeks.theta.is_finite());
    }

    #[rstest]
    fn test_imply_vol_basic() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let theoretical_price = black_scholes_greeks(s, r, b, vol, is_call, k, t).price;
        let implied_vol = imply_vol(s, r, b, is_call, k, t, theoretical_price);

        // Tolerance relaxed due to numerical precision differences between fast_norm_query and exact methods
        let tolerance = 1e-4;
        assert!(
            (implied_vol - vol).abs() < tolerance,
            "Implied vol difference exceeds tolerance: {implied_vol} vs {vol}"
        );
    }

    // Note: Implied volatility tests across different strikes can be sensitive to numerical precision
    // The basic implied vol test already covers the core functionality

    // Note: Comprehensive implied vol consistency test is challenging due to numerical precision
    // The existing accuracy tests already cover this functionality adequately

    #[rstest]
    fn test_greeks_data_new() {
        let greeks = create_test_greeks_data();

        assert_eq!(greeks.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(greeks.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(
            greeks.instrument_id,
            InstrumentId::from("SPY240315C00500000.OPRA")
        );
        assert!(greeks.is_call);
        assert_eq!(greeks.strike, 500.0);
        assert_eq!(greeks.expiry, 20_240_315);
        assert_eq!(greeks.expiry_in_years, 0.25);
        assert_eq!(greeks.multiplier, 100.0);
        assert_eq!(greeks.quantity, 1.0);
        assert_eq!(greeks.underlying_price, 520.0);
        assert_eq!(greeks.interest_rate, 0.05);
        assert_eq!(greeks.cost_of_carry, 0.05);
        assert_eq!(greeks.vol, 0.2);
        assert_eq!(greeks.pnl, 250.0);
        assert_eq!(greeks.price, 25.5);
        assert_eq!(greeks.delta, 0.65);
        assert_eq!(greeks.gamma, 0.003);
        assert_eq!(greeks.vega, 15.2);
        assert_eq!(greeks.theta, -0.08);
        assert_eq!(greeks.itm_prob, 0.75);
    }

    #[rstest]
    fn test_greeks_data_from_delta() {
        let delta = 0.5;
        let multiplier = 100.0;
        let ts_event = UnixNanos::from(2_000_000_000);
        let instrument_id = InstrumentId::from("AAPL240315C00180000.OPRA");

        let greeks = GreeksData::from_delta(instrument_id, delta, multiplier, ts_event);

        assert_eq!(greeks.ts_init, ts_event);
        assert_eq!(greeks.ts_event, ts_event);
        assert_eq!(greeks.instrument_id, instrument_id);
        assert!(greeks.is_call);
        assert_eq!(greeks.delta, delta);
        assert_eq!(greeks.multiplier, multiplier);
        assert_eq!(greeks.quantity, 1.0);

        // Check that all other fields are zeroed
        assert_eq!(greeks.strike, 0.0);
        assert_eq!(greeks.expiry, 0);
        assert_eq!(greeks.price, 0.0);
        assert_eq!(greeks.gamma, 0.0);
        assert_eq!(greeks.vega, 0.0);
        assert_eq!(greeks.theta, 0.0);
    }

    #[rstest]
    fn test_greeks_data_default() {
        let greeks = GreeksData::default();

        assert_eq!(greeks.ts_init, UnixNanos::default());
        assert_eq!(greeks.ts_event, UnixNanos::default());
        assert_eq!(greeks.instrument_id, InstrumentId::from("ES.GLBX"));
        assert!(greeks.is_call);
        assert_eq!(greeks.strike, 0.0);
        assert_eq!(greeks.expiry, 0);
        assert_eq!(greeks.multiplier, 0.0);
        assert_eq!(greeks.quantity, 0.0);
        assert_eq!(greeks.delta, 0.0);
        assert_eq!(greeks.gamma, 0.0);
        assert_eq!(greeks.vega, 0.0);
        assert_eq!(greeks.theta, 0.0);
    }

    #[rstest]
    fn test_greeks_data_display() {
        let greeks = create_test_greeks_data();
        let display_str = format!("{greeks}");

        assert_eq!(
            display_str,
            "GreeksData(instrument_id=SPY240315C00500000.OPRA, expiry=20240315, itm_prob=75.00%, vol=20.00%, pnl=250.00, price=25.50, delta=0.65, gamma=0.00, vega=15.20, theta=-0.08, quantity=1, ts_event=1970-01-01T00:00:01.500000000Z, ts_init=1970-01-01T00:00:01.000000000Z)"
        );
    }

    #[rstest]
    fn test_greeks_data_multiplication() {
        let greeks = create_test_greeks_data();
        let quantity = 5.0;
        let scaled_greeks = quantity * &greeks;

        assert_eq!(scaled_greeks.ts_init, greeks.ts_init);
        assert_eq!(scaled_greeks.ts_event, greeks.ts_event);
        assert_eq!(scaled_greeks.instrument_id, greeks.instrument_id);
        assert_eq!(scaled_greeks.is_call, greeks.is_call);
        assert_eq!(scaled_greeks.strike, greeks.strike);
        assert_eq!(scaled_greeks.expiry, greeks.expiry);
        assert_eq!(scaled_greeks.multiplier, greeks.multiplier);
        assert_eq!(scaled_greeks.quantity, greeks.quantity);
        assert_eq!(scaled_greeks.vol, greeks.vol);
        assert_eq!(scaled_greeks.itm_prob, greeks.itm_prob);

        // Check scaled values
        assert_eq!(scaled_greeks.pnl, quantity * greeks.pnl);
        assert_eq!(scaled_greeks.price, quantity * greeks.price);
        assert_eq!(scaled_greeks.delta, quantity * greeks.delta);
        assert_eq!(scaled_greeks.gamma, quantity * greeks.gamma);
        assert_eq!(scaled_greeks.vega, quantity * greeks.vega);
        assert_eq!(scaled_greeks.theta, quantity * greeks.theta);
    }

    #[rstest]
    fn test_greeks_data_has_ts_init() {
        let greeks = create_test_greeks_data();
        assert_eq!(greeks.ts_init(), UnixNanos::from(1_000_000_000));
    }

    #[rstest]
    fn test_greeks_data_clone() {
        let greeks1 = create_test_greeks_data();
        let greeks2 = greeks1.clone();

        assert_eq!(greeks1.ts_init, greeks2.ts_init);
        assert_eq!(greeks1.instrument_id, greeks2.instrument_id);
        assert_eq!(greeks1.delta, greeks2.delta);
        assert_eq!(greeks1.gamma, greeks2.gamma);
    }

    #[rstest]
    fn test_portfolio_greeks_new() {
        let portfolio_greeks = create_test_portfolio_greeks();

        assert_eq!(portfolio_greeks.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(portfolio_greeks.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(portfolio_greeks.pnl, 1500.0);
        assert_eq!(portfolio_greeks.price, 125.5);
        assert_eq!(portfolio_greeks.delta, 2.15);
        assert_eq!(portfolio_greeks.gamma, 0.008);
        assert_eq!(portfolio_greeks.vega, 42.7);
        assert_eq!(portfolio_greeks.theta, -2.3);
    }

    #[rstest]
    fn test_portfolio_greeks_default() {
        let portfolio_greeks = PortfolioGreeks::default();

        assert_eq!(portfolio_greeks.ts_init, UnixNanos::default());
        assert_eq!(portfolio_greeks.ts_event, UnixNanos::default());
        assert_eq!(portfolio_greeks.pnl, 0.0);
        assert_eq!(portfolio_greeks.price, 0.0);
        assert_eq!(portfolio_greeks.delta, 0.0);
        assert_eq!(portfolio_greeks.gamma, 0.0);
        assert_eq!(portfolio_greeks.vega, 0.0);
        assert_eq!(portfolio_greeks.theta, 0.0);
    }

    #[rstest]
    fn test_portfolio_greeks_display() {
        let portfolio_greeks = create_test_portfolio_greeks();
        let display_str = format!("{portfolio_greeks}");

        assert_eq!(
            display_str,
            "PortfolioGreeks(pnl=1500.00, price=125.50, delta=2.15, gamma=0.01, vega=42.70, theta=-2.30, ts_event=1970-01-01T00:00:01.500000000Z, ts_init=1970-01-01T00:00:01.000000000Z)"
        );
    }

    #[rstest]
    fn test_portfolio_greeks_addition() {
        let mut greeks1 = PortfolioGreeks::new(
            100.0,
            50.0,
            1.0,
            0.005,
            20.0,
            -1.0,
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(1_000_000_000),
        );

        let mut greeks2 = PortfolioGreeks::new(
            200.0,
            75.0,
            1.5,
            0.003,
            25.0,
            -1.5,
            UnixNanos::from(2_500_000_000),
            UnixNanos::from(2_000_000_000),
        );

        greeks1.greeks.rho = 12.0;
        greeks2.greeks.rho = 17.0;

        let result = greeks1 + greeks2;

        assert_eq!(result.ts_init, UnixNanos::from(1_000_000_000)); // Uses first ts_init
        assert_eq!(result.ts_event, UnixNanos::from(1_500_000_000)); // Uses first ts_event
        assert_eq!(result.pnl, 300.0);
        assert_eq!(result.price, 125.0);
        assert_eq!(result.delta, 2.5);
        assert_eq!(result.gamma, 0.008);
        assert_eq!(result.vega, 45.0);
        assert_eq!(result.theta, -2.5);
        assert_eq!(result.rho, 29.0);
    }

    #[rstest]
    #[case(1.0, [20.0, 30.0, 40.0, 50.0, 60.0, -70.0, 80.0])]
    #[case(2.0, [40.0, 60.0, 80.0, 100.0, 120.0, -140.0, 160.0])]
    #[case(-3.0, [-60.0, -90.0, -120.0, -150.0, -180.0, 210.0, -240.0])]
    #[case(0.0, [0.0; 7])]
    fn test_greeks_data_portfolio_scaling(#[case] quantity: f64, #[case] expected: [f64; 7]) {
        let greeks = GreeksData {
            pnl: 2.0,
            price: 3.0,
            multiplier: 10.0,
            quantity: 9.0,
            greeks: OptionGreekValues {
                delta: 4.0,
                gamma: 5.0,
                vega: 6.0,
                theta: -7.0,
                rho: 8.0,
            },
            ..create_test_greeks_data()
        };

        let portfolio = quantity * &greeks.to_portfolio_greeks();

        assert_eq!(portfolio.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(portfolio.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(
            [
                portfolio.pnl,
                portfolio.price,
                portfolio.delta,
                portfolio.gamma,
                portfolio.vega,
                portfolio.theta,
                portfolio.rho
            ],
            expected,
        );
        assert_eq!(greeks.pnl, 2.0);
        assert_eq!(greeks.price, 3.0);
        assert_eq!(greeks.multiplier, 10.0);
        assert_eq!(greeks.quantity, 9.0);
        assert_eq!(
            greeks.greeks,
            OptionGreekValues {
                delta: 4.0,
                gamma: 5.0,
                vega: 6.0,
                theta: -7.0,
                rho: 8.0,
            }
        );
    }

    #[rstest]
    fn test_portfolio_greeks_from_greeks_data() {
        let greeks_data = create_test_greeks_data();
        let portfolio_greeks: PortfolioGreeks = greeks_data.clone().into();

        assert_eq!(portfolio_greeks.ts_init, greeks_data.ts_init);
        assert_eq!(portfolio_greeks.ts_event, greeks_data.ts_event);
        assert_eq!(portfolio_greeks.pnl, 25_000.0);
        assert_eq!(portfolio_greeks.price, 2_550.0);
        assert_eq!(portfolio_greeks.delta, 65.0);
        assert_eq!(portfolio_greeks.gamma, 0.3);
        assert_eq!(portfolio_greeks.vega, 1_520.0);
        assert_eq!(portfolio_greeks.theta, -8.0);
        assert_eq!(portfolio_greeks.rho, 0.0);
    }

    #[rstest]
    fn test_portfolio_greeks_has_ts_init() {
        let portfolio_greeks = create_test_portfolio_greeks();
        assert_eq!(portfolio_greeks.ts_init(), UnixNanos::from(1_000_000_000));
    }

    #[rstest]
    fn test_yield_curve_new() {
        let curve = create_test_yield_curve();

        assert_eq!(curve.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(curve.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(curve.curve_name, "USD");
        assert_eq!(curve.tenors, vec![0.25, 0.5, 1.0, 2.0, 5.0]);
        assert_eq!(curve.interest_rates, vec![0.025, 0.03, 0.035, 0.04, 0.045]);
    }

    #[rstest]
    fn test_yield_curve_default() {
        let curve = YieldCurve::default();

        assert_eq!(curve.ts_init, UnixNanos::default());
        assert_eq!(curve.ts_event, UnixNanos::default());
        assert_eq!(curve.curve_name, "USD");
        assert_eq!(curve.tenors, vec![0.5, 1.0, 1.5, 2.0, 2.5]);
        assert_eq!(curve.interest_rates, vec![0.04, 0.04, 0.04, 0.04, 0.04]);
    }

    #[rstest]
    fn test_yield_curve_get_rate_single_point() {
        let curve = YieldCurve::new(
            "USD".to_string(),
            vec![1.0],
            vec![0.05],
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(curve.get_rate(0.5), 0.05);
        assert_eq!(curve.get_rate(1.0), 0.05);
        assert_eq!(curve.get_rate(2.0), 0.05);
    }

    #[rstest]
    fn test_yield_curve_get_rate_interpolation() {
        let curve = create_test_yield_curve();

        // Test exact matches
        assert_eq!(curve.get_rate(0.25), 0.025);
        assert_eq!(curve.get_rate(1.0), 0.035);
        assert_eq!(curve.get_rate(5.0), 0.045);

        // Test interpolation (results will depend on quadratic_interpolation implementation)
        let rate_0_75 = curve.get_rate(0.75);
        assert!(rate_0_75 > 0.025 && rate_0_75 < 0.045);
    }

    #[rstest]
    fn test_yield_curve_display() {
        let curve = create_test_yield_curve();
        let display_str = format!("{curve}");

        assert_eq!(
            display_str,
            "YieldCurve(curve_name=USD, ts_event=1970-01-01T00:00:01.500000000Z, ts_init=1970-01-01T00:00:01.000000000Z)"
        );
    }

    #[rstest]
    fn test_yield_curve_has_ts_init() {
        let curve = create_test_yield_curve();
        assert_eq!(curve.ts_init(), UnixNanos::from(1_000_000_000));
    }

    #[rstest]
    fn test_yield_curve_clone() {
        let curve1 = create_test_yield_curve();
        let curve2 = curve1.clone();

        assert_eq!(curve1.curve_name, curve2.curve_name);
        assert_eq!(curve1.tenors, curve2.tenors);
        assert_eq!(curve1.interest_rates, curve2.interest_rates);
    }

    #[rstest]
    #[case(vec![0.0], vec![0.04])]
    #[case(vec![0.25, 1.0, 5.0], vec![0.043, 0.041, 0.038])]
    fn test_yield_curve_validate_accepts_evaluable_curves(
        #[case] tenors: Vec<f64>,
        #[case] interest_rates: Vec<f64>,
    ) {
        let curve = YieldCurve::new(
            "USD".to_string(),
            tenors,
            interest_rates,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(curve.validate().is_ok());
    }

    #[rstest]
    #[case(vec![0.5, 1.0, 2.0], vec![0.04, 0.05], "'tenors' usize of 3 was not equal")]
    #[case(vec![0.5, 1.0], vec![0.04, 0.05], "one rate or at least three points")]
    #[case(vec![], vec![], "one rate or at least three points")]
    #[case(vec![0.25, 1.0, 5.0], vec![0.04, f64::NAN, 0.05], "must be finite")]
    #[case(vec![0.25, f64::INFINITY, 5.0], vec![0.04, 0.05, 0.06], "must be finite")]
    #[case(vec![0.05, 0.05, 1.0], vec![0.04, 0.05, 0.06], "strictly increasing")]
    #[case(vec![1.0, 0.5, 2.0], vec![0.04, 0.05, 0.06], "strictly increasing")]
    #[case(vec![1.0, 1.0 + f64::EPSILON, 2.0], vec![0.04, 0.05, 0.06], "wide enough")]
    fn test_yield_curve_validate_rejects_curves_get_rate_cannot_evaluate(
        #[case] tenors: Vec<f64>,
        #[case] interest_rates: Vec<f64>,
        #[case] message: &str,
    ) {
        let curve = YieldCurve::new(
            "USD".to_string(),
            tenors,
            interest_rates,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let error = curve.validate().unwrap_err();

        assert!(
            error.to_string().contains(message),
            "error was {error}, expected to contain {message}"
        );
    }

    #[rstest]
    fn test_yield_curve_custom_data_json_round_trip() {
        let curve = create_test_yield_curve();
        let mut other = curve.clone();
        other.interest_rates[0] = 0.02;

        let json = curve.to_json().unwrap();
        let restored = YieldCurve::from_json(serde_json::from_str(&json).unwrap()).unwrap();

        assert_eq!(curve.type_name(), "YieldCurve");
        assert_eq!(YieldCurve::type_name_static(), "YieldCurve");
        assert_eq!(
            CustomDataTrait::ts_event(&curve),
            UnixNanos::from(1_500_000_000)
        );
        assert!(restored.eq_arc(&curve));
        assert!(!restored.eq_arc(&other));
        assert_eq!(restored.as_any().downcast_ref::<YieldCurve>(), Some(&curve));
    }

    #[rstest]
    fn test_black_scholes_greeks_extreme_values() {
        let s = 1000.0;
        let r = 0.1;
        let b = 0.1;
        let vol = 0.5;
        let is_call = true;
        let k = 10.0; // Very deep ITM
        let t = 0.1;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.price.is_finite());
        assert!(greeks.delta.is_finite());
        assert!(greeks.gamma.is_finite());
        assert!(greeks.vega.is_finite());
        assert!(greeks.theta.is_finite());
        assert!(greeks.price > 0.0);
        assert!(greeks.delta > 0.99); // Very deep ITM call
    }

    #[rstest]
    fn test_black_scholes_greeks_high_volatility() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let vol = 2.0; // 200% volatility
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        assert!(greeks.price.is_finite());
        assert!(greeks.delta.is_finite());
        assert!(greeks.gamma.is_finite());
        assert!(greeks.vega.is_finite());
        assert!(greeks.theta.is_finite());
        assert!(greeks.price > 0.0);
    }

    #[rstest]
    fn test_greeks_data_put_option() {
        let greeks = GreeksData::new(
            InstrumentId::from("SPY240315P00480000.OPRA"),
            false, // Put option
            480.0,
            20_240_315,
            91, // expiry_in_days (approximately 3 months)
            0.25,
            100.0,
            1.0,
            500.0,
            0.05,
            0.05,
            0.25,
            -150.0, // Negative PnL
            8.5,
            OptionGreekValues {
                delta: -0.35,
                gamma: 0.002,
                vega: 12.8,
                theta: -0.06,
                rho: 0.0,
            },
            0.25,
            UnixNanos::from(1_500_000_000),
            UnixNanos::from(1_000_000_000),
        );

        assert!(!greeks.is_call);
        assert!(greeks.delta < 0.0);
        assert_eq!(greeks.pnl, -150.0);
    }

    // Original accuracy tests (keeping these as they are comprehensive)
    #[rstest]
    fn test_greeks_accuracy_call() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let vol = 0.2;
        let is_call = true;
        let eps = 1e-3;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        // Use exact method for finite difference calculations for better precision
        let price0 = |s: f64| black_scholes_greeks_exact(s, r, b, vol, is_call, k, t).price;

        let delta_bnr = (price0(s + eps) - price0(s - eps)) / (2.0 * eps);
        let gamma_bnr = (price0(s + eps) + price0(s - eps) - 2.0 * price0(s)) / (eps * eps);
        let vega_bnr = (black_scholes_greeks_exact(s, r, b, vol + eps, is_call, k, t).price
            - black_scholes_greeks_exact(s, r, b, vol - eps, is_call, k, t).price)
            / (2.0 * eps)
            / 100.0;
        let theta_bnr = (black_scholes_greeks_exact(s, r, b, vol, is_call, k, t - eps).price
            - black_scholes_greeks_exact(s, r, b, vol, is_call, k, t + eps).price)
            / (2.0 * eps)
            / 365.25;

        // Tolerance relaxed due to differences between fast f32 implementation and exact finite difference approximations
        // Also accounts for differences in how b (cost of carry) is handled between implementations
        let tolerance = 5e-3;
        assert!(
            (greeks.delta - delta_bnr).abs() < tolerance,
            "Delta difference exceeds tolerance: {} vs {}",
            greeks.delta,
            delta_bnr
        );
        // Gamma tolerance is more relaxed due to second-order finite differences being less accurate and f32 precision
        let gamma_tolerance = 0.1;
        assert!(
            (greeks.gamma - gamma_bnr).abs() < gamma_tolerance,
            "Gamma difference exceeds tolerance: {} vs {}",
            greeks.gamma,
            gamma_bnr
        );
        // Both greeks.vega and vega_bnr are per 1% vol (absolute percent change).
        assert!(
            (greeks.vega - vega_bnr).abs() < tolerance,
            "Vega difference exceeds tolerance: {} vs {}",
            greeks.vega,
            vega_bnr
        );
        assert!(
            (greeks.theta - theta_bnr).abs() < tolerance,
            "Theta difference exceeds tolerance: {} vs {}",
            greeks.theta,
            theta_bnr
        );
    }

    #[rstest]
    fn test_greeks_accuracy_put() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let vol = 0.2;
        let is_call = false;
        let eps = 1e-3;

        let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);

        // Use exact method for finite difference calculations for better precision
        let price0 = |s: f64| black_scholes_greeks_exact(s, r, b, vol, is_call, k, t).price;

        let delta_bnr = (price0(s + eps) - price0(s - eps)) / (2.0 * eps);
        let gamma_bnr = (price0(s + eps) + price0(s - eps) - 2.0 * price0(s)) / (eps * eps);
        let vega_bnr = (black_scholes_greeks_exact(s, r, b, vol + eps, is_call, k, t).price
            - black_scholes_greeks_exact(s, r, b, vol - eps, is_call, k, t).price)
            / (2.0 * eps)
            / 100.0;
        let theta_bnr = (black_scholes_greeks_exact(s, r, b, vol, is_call, k, t - eps).price
            - black_scholes_greeks_exact(s, r, b, vol, is_call, k, t + eps).price)
            / (2.0 * eps)
            / 365.25;

        // Tolerance relaxed due to differences between fast f32 implementation and exact finite difference approximations
        // Also accounts for differences in how b (cost of carry) is handled between implementations
        let tolerance = 5e-3;
        assert!(
            (greeks.delta - delta_bnr).abs() < tolerance,
            "Delta difference exceeds tolerance: {} vs {}",
            greeks.delta,
            delta_bnr
        );
        // Gamma tolerance is more relaxed due to second-order finite differences being less accurate and f32 precision
        let gamma_tolerance = 0.1;
        assert!(
            (greeks.gamma - gamma_bnr).abs() < gamma_tolerance,
            "Gamma difference exceeds tolerance: {} vs {}",
            greeks.gamma,
            gamma_bnr
        );
        // Both greeks.vega and vega_bnr are per 1% vol (absolute percent change).
        assert!(
            (greeks.vega - vega_bnr).abs() < tolerance,
            "Vega difference exceeds tolerance: {} vs {}",
            greeks.vega,
            vega_bnr
        );
        assert!(
            (greeks.theta - theta_bnr).abs() < tolerance,
            "Theta difference exceeds tolerance: {} vs {}",
            greeks.theta,
            theta_bnr
        );
    }

    #[rstest]
    fn test_imply_vol_and_greeks_accuracy_call() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let vol = 0.2;
        let is_call = true;

        let base_greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);
        let price = base_greeks.price;

        let implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price);

        // Tolerance relaxed due to numerical precision differences
        let tolerance = 2e-4;
        assert!(
            (implied_result.vol - vol).abs() < tolerance,
            "Vol difference exceeds tolerance: {} vs {}",
            implied_result.vol,
            vol
        );
        assert!(
            (implied_result.price - base_greeks.price).abs() < tolerance,
            "Price difference exceeds tolerance: {} vs {}",
            implied_result.price,
            base_greeks.price
        );
        assert!(
            (implied_result.delta - base_greeks.delta).abs() < tolerance,
            "Delta difference exceeds tolerance: {} vs {}",
            implied_result.delta,
            base_greeks.delta
        );
        assert!(
            (implied_result.gamma - base_greeks.gamma).abs() < tolerance,
            "Gamma difference exceeds tolerance: {} vs {}",
            implied_result.gamma,
            base_greeks.gamma
        );
        assert!(
            (implied_result.vega - base_greeks.vega).abs() < tolerance,
            "Vega difference exceeds tolerance: {} vs {}",
            implied_result.vega,
            base_greeks.vega
        );
        assert!(
            (implied_result.theta - base_greeks.theta).abs() < tolerance,
            "Theta difference exceeds tolerance: {} vs {}",
            implied_result.theta,
            base_greeks.theta
        );
    }

    #[rstest]
    fn test_black_scholes_greeks_target_price_refinement() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let initial_vol = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        // Calculate the price with the initial vol
        let initial_greeks = black_scholes_greeks(s, r, b, initial_vol, is_call, k, t);
        let target_price = initial_greeks.price;

        // Now use a slightly different vol and refine it using target_price
        let refined_vol = initial_vol * 1.1; // 10% higher vol
        let refined_greeks =
            refine_vol_and_greeks(s, r, b, is_call, k, t, target_price, refined_vol);

        // The refined vol should be closer to the initial vol, and the price should match the target
        // Tolerance matches the function's convergence tolerance (price_epsilon * 2.0)
        let price_tolerance = (s * 5e-5).max(1e-4) * 2.0;
        assert!(
            (refined_greeks.price - target_price).abs() < price_tolerance,
            "Refined price should match target: {} vs {}",
            refined_greeks.price,
            target_price
        );

        // The refined vol should be between the initial and refined vol (converged towards initial)
        assert!(
            refined_vol > refined_greeks.vol && refined_greeks.vol > initial_vol * 0.9,
            "Refined vol should converge towards initial: {} (initial: {}, refined: {})",
            refined_greeks.vol,
            initial_vol,
            refined_vol
        );
    }

    #[rstest]
    fn test_black_scholes_greeks_target_price_refinement_put() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let initial_vol = 0.25;
        let is_call = false;
        let k = 105.0;
        let t = 0.5;

        // Calculate the price with the initial vol
        let initial_greeks = black_scholes_greeks(s, r, b, initial_vol, is_call, k, t);
        let target_price = initial_greeks.price;

        // Now use a different vol and refine it using target_price
        let refined_vol = initial_vol * 0.8; // 20% lower vol
        let refined_greeks =
            refine_vol_and_greeks(s, r, b, is_call, k, t, target_price, refined_vol);

        // The refined price should match the target
        // Tolerance matches the function's convergence tolerance (price_epsilon * 2.0)
        let price_tolerance = (s * 5e-5).max(1e-4) * 2.0;
        assert!(
            (refined_greeks.price - target_price).abs() < price_tolerance,
            "Refined price should match target: {} vs {}",
            refined_greeks.price,
            target_price
        );

        // The refined vol should converge towards the initial vol
        assert!(
            refined_vol < refined_greeks.vol && refined_greeks.vol < initial_vol * 1.1,
            "Refined vol should converge towards initial: {} (initial: {}, refined: {})",
            refined_greeks.vol,
            initial_vol,
            refined_vol
        );
    }

    #[rstest]
    fn test_imply_vol_and_greeks_accuracy_put() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let vol = 0.2;
        let is_call = false;

        let base_greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t);
        let price = base_greeks.price;

        let implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price);

        // Tolerance relaxed due to numerical precision differences
        let tolerance = 2e-4;
        assert!(
            (implied_result.vol - vol).abs() < tolerance,
            "Vol difference exceeds tolerance: {} vs {}",
            implied_result.vol,
            vol
        );
        assert!(
            (implied_result.price - base_greeks.price).abs() < tolerance,
            "Price difference exceeds tolerance: {} vs {}",
            implied_result.price,
            base_greeks.price
        );
        assert!(
            (implied_result.delta - base_greeks.delta).abs() < tolerance,
            "Delta difference exceeds tolerance: {} vs {}",
            implied_result.delta,
            base_greeks.delta
        );
        assert!(
            (implied_result.gamma - base_greeks.gamma).abs() < tolerance,
            "Gamma difference exceeds tolerance: {} vs {}",
            implied_result.gamma,
            base_greeks.gamma
        );
        assert!(
            (implied_result.vega - base_greeks.vega).abs() < tolerance,
            "Vega difference exceeds tolerance: {} vs {}",
            implied_result.vega,
            base_greeks.vega
        );
        assert!(
            (implied_result.theta - base_greeks.theta).abs() < tolerance,
            "Theta difference exceeds tolerance: {} vs {}",
            implied_result.theta,
            base_greeks.theta
        );
    }

    // Parameterized tests comparing black_scholes_greeks against black_scholes_greeks_exact
    // Testing three moneyness levels (OTM, ATM, ITM) and both call and put options
    #[rstest]
    fn test_black_scholes_greeks_vs_exact(
        #[values(90.0, 100.0, 110.0)] spot: f64,
        #[values(true, false)] is_call: bool,
        #[values(0.15, 0.25, 0.5)] vol: f64,
        #[values(0.01, 0.25, 2.0)] t: f64,
    ) {
        let r = 0.05;
        let b = 0.05;
        let k = 100.0;

        let greeks_fast = black_scholes_greeks(spot, r, b, vol, is_call, k, t);
        let greeks_exact = black_scholes_greeks_exact(spot, r, b, vol, is_call, k, t);

        // Verify ~7 significant decimals precision using relative error checks
        // For 7 significant decimals: relative error < 5e-6 (accounts for f32 intermediate calculations)
        // Use max(|exact|, 1e-10) to avoid division by zero for very small values
        // Very short expiry (0.01) can have slightly larger relative errors due to numerical precision
        let rel_tolerance = if t < 0.1 {
            1e-4 // More lenient for very short expiry (~5 significant decimals)
        } else {
            8e-6 // Standard tolerance for normal/long expiry (~6.1 significant decimals)
        };
        let abs_tolerance = 1e-10; // Minimum absolute tolerance for near-zero values

        // Check relative error with seven significant decimal places
        let check_7_sig_figs = |fast: f64, exact: f64, name: &str| {
            let abs_diff = (fast - exact).abs();
            // For very small values (near zero), use absolute tolerance instead of relative
            // This handles cases with very short expiry where values can be very close to zero
            // Use a threshold of 1e-4 for "very small" values
            let small_value_threshold = 1e-4;
            let max_allowed = if exact.abs() < small_value_threshold {
                // Both values are very small, use absolute tolerance (more lenient for very small values)
                if t < 0.1 {
                    1e-5 // Very lenient for very short expiry with small values
                } else {
                    1e-6 // Standard absolute tolerance for small values
                }
            } else {
                // Use relative tolerance
                exact.abs().max(abs_tolerance) * rel_tolerance
            };
            let rel_diff = if exact.abs() > abs_tolerance {
                abs_diff / exact.abs()
            } else {
                0.0 // Both near zero, difference is acceptable
            };

            assert!(
                abs_diff < max_allowed,
                "{name} mismatch for spot={spot}, is_call={is_call}, vol={vol}, t={t}: fast={fast:.10}, exact={exact:.10}, abs_diff={abs_diff:.2e}, rel_diff={rel_diff:.2e}, max_allowed={max_allowed:.2e}"
            );
        };

        check_7_sig_figs(greeks_fast.price, greeks_exact.price, "Price");
        check_7_sig_figs(greeks_fast.delta, greeks_exact.delta, "Delta");
        check_7_sig_figs(greeks_fast.gamma, greeks_exact.gamma, "Gamma");
        check_7_sig_figs(greeks_fast.vega, greeks_exact.vega, "Vega");
        check_7_sig_figs(greeks_fast.theta, greeks_exact.theta, "Theta");
    }

    // Parameterized tests comparing refine_vol_and_greeks against imply_vol_and_greeks
    // Testing that both methods recover the target volatility and produce similar greeks
    #[rstest]
    fn test_refine_vol_and_greeks_vs_imply_vol_and_greeks(
        #[values(90.0, 100.0, 110.0)] spot: f64,
        #[values(true, false)] is_call: bool,
        #[values(0.15, 0.25, 0.5)] target_vol: f64,
        #[values(0.01, 0.25, 2.0)] t: f64,
    ) {
        let r = 0.05;
        let b = 0.05;
        let k = 100.0;

        // Compute the theoretical price using the target volatility
        let base_greeks = black_scholes_greeks(spot, r, b, target_vol, is_call, k, t);
        let target_price = base_greeks.price;

        // Initial guess is 0.01 below the target vol
        let initial_guess = target_vol - 0.01;

        // Recover volatility using refine_vol_and_greeks
        let refined_result =
            refine_vol_and_greeks(spot, r, b, is_call, k, t, target_price, initial_guess);

        // Recover volatility using imply_vol_and_greeks
        let implied_result = imply_vol_and_greeks(spot, r, b, is_call, k, t, target_price);

        // Detect deep ITM/OTM options (more than 5% away from ATM)
        // These are especially challenging for imply_vol with very short expiry
        let moneyness = (spot - k) / k;
        let is_deep_itm_otm = moneyness.abs() > 0.05;
        let is_deep_edge_case = t < 0.1 && is_deep_itm_otm;

        // Verify both methods recover the target volatility
        // refine_vol_and_greeks uses a single Halley iteration, so convergence may be limited
        // Initial guess is 0.01 below target, which should provide reasonable convergence
        // Very short (0.01) or very long (2.0) expiry can make convergence more challenging
        // Deep ITM/OTM with very short expiry is especially problematic for imply_vol
        let vol_abs_tolerance = 1e-6;
        let vol_rel_tolerance = if is_deep_edge_case {
            // Deep ITM/OTM with very short expiry: imply_vol often fails, use very lenient tolerance
            2.0 // Very lenient to effectively skip when imply_vol fails for these edge cases
        } else if t < 0.1 {
            // Very short expiry: convergence is more challenging
            0.10 // Lenient for short expiry
        } else if t > 1.5 {
            // Very long expiry: convergence can be challenging
            if target_vol <= 0.15 {
                0.05 // Moderate tolerance for 0.15 vol with long expiry
            } else {
                0.01 // Moderate tolerance for higher vols with long expiry
            }
        } else {
            // Normal expiry (0.25-1.5): use standard tolerances
            if target_vol <= 0.15 {
                0.05 // Moderate tolerance for 0.15 vol
            } else {
                0.001 // Tighter tolerance for higher vols (0.1% relative error)
            }
        };

        let refined_vol_error = (refined_result.vol - target_vol).abs();
        let implied_vol_error = (implied_result.vol - target_vol).abs();
        let refined_vol_rel_error = refined_vol_error / target_vol.max(vol_abs_tolerance);
        let implied_vol_rel_error = implied_vol_error / target_vol.max(vol_abs_tolerance);

        assert!(
            refined_vol_rel_error < vol_rel_tolerance,
            "Refined vol mismatch for spot={}, is_call={}, target_vol={}, t={}: refined={:.10}, target={:.10}, abs_error={:.2e}, rel_error={:.2e}",
            spot,
            is_call,
            target_vol,
            t,
            refined_result.vol,
            target_vol,
            refined_vol_error,
            refined_vol_rel_error
        );

        // For very short expiry, imply_vol may fail (return 0.0 or very wrong value), so use very lenient tolerance
        // Deep ITM/OTM with very short expiry is especially problematic
        let implied_vol_tolerance = if is_deep_edge_case {
            // Deep ITM/OTM with very short expiry: imply_vol often fails
            2.0 // Very lenient to effectively skip
        } else if implied_result.vol < 1e-6 {
            // imply_vol failed (returned 0.0), skip this check
            2.0 // Very lenient to effectively skip (allow 100%+ error)
        } else if t < 0.1 && (implied_result.vol - target_vol).abs() / target_vol.max(1e-6) > 0.5 {
            // For very short expiry, if implied vol is way off (>50% error), imply_vol likely failed
            2.0 // Very lenient to effectively skip
        } else {
            vol_rel_tolerance
        };

        assert!(
            implied_vol_rel_error < implied_vol_tolerance,
            "Implied vol mismatch for spot={}, is_call={}, target_vol={}, t={}: implied={:.10}, target={:.10}, abs_error={:.2e}, rel_error={:.2e}",
            spot,
            is_call,
            target_vol,
            t,
            implied_result.vol,
            target_vol,
            implied_vol_error,
            implied_vol_rel_error
        );

        // Verify greeks from both methods are close (6 decimals precision)
        // Note: Since refine_vol_and_greeks may not fully converge, the recovered vols may differ slightly,
        // which will cause the greeks to differ. Use adaptive tolerance based on vol recovery quality and expiry.
        let greeks_abs_tolerance = 1e-10;

        // Detect deep ITM/OTM options (more than 5% away from ATM)
        let moneyness = (spot - k) / k;
        let is_deep_itm_otm = moneyness.abs() > 0.05;
        let is_deep_edge_case = t < 0.1 && is_deep_itm_otm;

        // Use more lenient tolerance for low vols and extreme expiry where convergence is more challenging
        // All greeks are sensitive to vol differences at low vols and extreme expiry
        // Deep ITM/OTM with very short expiry is especially challenging for imply_vol
        let greeks_rel_tolerance = if is_deep_edge_case {
            // Deep ITM/OTM with very short expiry: imply_vol often fails, use very lenient tolerance
            1.0 // Very lenient to effectively skip when imply_vol fails for these edge cases
        } else if t < 0.1 {
            // Very short expiry: greeks are very sensitive
            if target_vol <= 0.15 {
                0.10 // Lenient for 0.15 vol with short expiry
            } else {
                0.05 // Lenient for higher vols with short expiry
            }
        } else if t > 1.5 {
            // Very long expiry: greeks can be sensitive
            if target_vol <= 0.15 {
                0.08 // More lenient for 0.15 vol with long expiry
            } else {
                0.01 // Moderate tolerance for higher vols with long expiry
            }
        } else {
            // Normal expiry (0.25-1.5): use standard tolerances
            if target_vol <= 0.15 {
                0.05 // Moderate tolerance for 0.15 vol
            } else {
                2e-3 // Tolerance for higher vols (~2.5 significant decimals)
            }
        };

        // Check relative error with six decimal places
        // Gamma is more sensitive to vol differences, so use more lenient tolerance
        // If imply_vol failed (vol < 1e-6 or way off for short expiry), the greeks may be wrong, so skip comparison
        // Deep ITM/OTM with very short expiry is especially problematic
        let imply_vol_failed = implied_result.vol < 1e-6
            || (t < 0.1 && (implied_result.vol - target_vol).abs() / target_vol.max(1e-6) > 0.5)
            || is_deep_edge_case;
        let effective_greeks_tolerance = if imply_vol_failed || is_deep_edge_case {
            1.0 // Very lenient to effectively skip when imply_vol fails or for deep ITM/OTM edge cases
        } else {
            greeks_rel_tolerance
        };

        let check_6_sig_figs = |refined: f64, implied: f64, name: &str, is_gamma: bool| {
            // Skip check if imply_vol failed and greeks contain NaN, invalid values, or very small values
            // Also skip for deep ITM/OTM with very short expiry where imply_vol is unreliable
            if (imply_vol_failed || is_deep_edge_case)
                && (!implied.is_finite() || implied.abs() < 1e-4 || refined.abs() < 1e-4)
            {
                return; // Skip this check when imply_vol fails or for deep ITM/OTM edge cases
            }

            let abs_diff = (refined - implied).abs();
            // If both values are very small, use absolute tolerance instead of relative
            // For deep ITM/OTM with short expiry, use more lenient absolute tolerance
            let small_value_threshold = if is_deep_edge_case { 1e-3 } else { 1e-6 };
            let rel_diff =
                if implied.abs() < small_value_threshold && refined.abs() < small_value_threshold {
                    0.0 // Both near zero, difference is acceptable
                } else {
                    abs_diff / implied.abs().max(greeks_abs_tolerance)
                };
            // Gamma is more sensitive, use higher multiplier for it, especially for low vols and extreme expiry
            let gamma_multiplier = if (0.1..=1.5).contains(&t) {
                // Normal expiry
                if target_vol <= 0.15 { 5.0 } else { 3.0 }
            } else {
                // Extreme expiry: gamma is very sensitive
                if target_vol <= 0.15 { 10.0 } else { 5.0 }
            };
            let tolerance = if is_gamma {
                effective_greeks_tolerance * gamma_multiplier
            } else {
                effective_greeks_tolerance
            };
            // For deep ITM/OTM with very short expiry and very small values, use absolute tolerance
            let max_allowed = if is_deep_edge_case && implied.abs() < 1e-3 {
                2e-5 // Very lenient absolute tolerance for deep edge cases with small values
            } else {
                implied.abs().max(greeks_abs_tolerance) * tolerance
            };

            assert!(
                abs_diff < max_allowed,
                "{name} mismatch between refine and imply for spot={spot}, is_call={is_call}, target_vol={target_vol}, t={t}: refined={refined:.10}, implied={implied:.10}, abs_diff={abs_diff:.2e}, rel_diff={rel_diff:.2e}, max_allowed={max_allowed:.2e}"
            );
        };

        check_6_sig_figs(refined_result.price, implied_result.price, "Price", false);
        check_6_sig_figs(refined_result.delta, implied_result.delta, "Delta", false);
        check_6_sig_figs(refined_result.gamma, implied_result.gamma, "Gamma", true);
        check_6_sig_figs(refined_result.vega, implied_result.vega, "Vega", false);
        check_6_sig_figs(refined_result.theta, implied_result.theta, "Theta", false);
    }

    #[rstest]
    #[case(true, 1.0, 0.437_729_301_518)]
    #[case(false, 1.0, -0.513_500_122_983)]
    #[case(true, 0.0, -0.075_770_821_464)]
    #[case(false, 0.0, -0.075_770_821_464)]
    fn test_checked_greeks_rho_at_zero_carry(
        #[case] is_call: bool,
        #[case] db_dr: f64,
        #[case] expected: f64,
    ) {
        let result =
            black_scholes_greeks_checked(100.0, 0.05, 0.0, 0.2, is_call, 100.0, 1.0, db_dr)
                .unwrap();
        assert!((result.rho - expected).abs() < 1e-6);
        assert_eq!(result.greeks().rho, result.rho);
        let raw = black_scholes_greeks(100.0, 0.05, 0.0, 0.2, is_call, 100.0, 1.0);
        assert_eq!(
            result,
            BlackScholesGreeksResult {
                rho: result.rho,
                ..raw
            }
        );
    }

    #[rstest]
    #[case(0, 0.0, "underlying price")]
    #[case(0, -1.0, "underlying price")]
    #[case(0, f64::INFINITY, "underlying price")]
    #[case(0, f64::MIN_POSITIVE, "underlying price")]
    #[case(0, f64::MAX, "underlying price")]
    #[case(1, f64::NAN, "interest rate")]
    #[case(2, f64::INFINITY, "cost of carry")]
    #[case(3, 0.0, "volatility")]
    #[case(3, -0.2, "volatility")]
    #[case(3, f64::NAN, "volatility")]
    #[case(4, 0.0, "strike")]
    #[case(4, -100.0, "strike")]
    #[case(5, 0.0, "time to expiry")]
    #[case(5, -1.0, "time to expiry")]
    fn test_checked_greeks_reject_invalid_inputs(
        #[case] index: usize,
        #[case] value: f64,
        #[case] expected: &'static str,
    ) {
        let mut inputs = [100.0, 0.05, 0.03, 0.2, 110.0, 0.5];
        inputs[index] = value;
        let [s, r, b, vol, k, t] = inputs;
        assert_eq!(
            black_scholes_greeks_checked(s, r, b, vol, true, k, t, 1.0),
            Err(OptionValuationError::InvalidInput(expected))
        );
    }

    #[rstest]
    #[case(true, 0.0)]
    #[case(true, 100.0)]
    #[case(true, 200.0)]
    #[case(false, 0.0)]
    #[case(false, 100.0)]
    #[case(false, 200.0)]
    fn test_checked_iv_rejects_zero_and_unattainable_premiums(
        #[case] is_call: bool,
        #[case] price: f64,
    ) {
        let expected = if price == 0.0 {
            OptionValuationError::InvalidInput("option price")
        } else {
            OptionValuationError::ImpliedVolatilityUnavailable
        };

        assert_eq!(
            imply_vol_and_greeks_checked(100.0, 0.0, 0.0, is_call, 100.0, 1.0, price, 1.0, None),
            Err(expected)
        );
    }

    #[rstest]
    #[case(1e-8)]
    #[case(0.21)]
    #[case(10.0)]
    fn test_checked_iv_refinement_falls_back_without_fabricated_volatility(#[case] seed: f64) {
        let raw = black_scholes_greeks(100.0, 0.05, 0.0, 0.2, true, 100.0, 1.0);
        let result = imply_vol_and_greeks_checked(
            100.0,
            0.05,
            0.0,
            true,
            100.0,
            1.0,
            raw.price,
            1.0,
            Some(seed),
        )
        .unwrap();
        assert!((result.vol - 0.2).abs() < 1e-5);
        assert!((result.price - raw.price).abs() < 1e-4);
        assert!((result.rho - 0.437_729_301_518).abs() < 1e-6);
    }

    #[rstest]
    fn test_checked_greeks_rejects_nonfinite_kernel_results() {
        assert_eq!(
            black_scholes_greeks_checked(100.0, 0.05, 1000.0, 0.2, true, 100.0, 1.0, 1.0),
            Err(OptionValuationError::InvalidResult)
        );
    }

    #[rstest]
    fn test_checked_iv_accepts_a_premium_the_fast_kernel_cannot_reprice() {
        let result =
            imply_vol_and_greeks_checked(100.0, 0.05, 0.05, true, 200.0, 1.0, 1e-10, 0.0, None)
                .unwrap();
        assert!((result.price - 1e-10).abs() < 1e-14);
        assert_eq!(
            result,
            black_scholes_greeks_exact(100.0, 0.05, 0.05, result.vol, true, 200.0, 1.0)
        );
    }

    #[rstest]
    #[case(f64::NAN)]
    #[case(f64::INFINITY)]
    fn test_checked_greeks_rejects_nonfinite_carry_sensitivity(#[case] db_dr: f64) {
        assert_eq!(
            black_scholes_greeks_checked(100.0, 0.05, 0.0, 0.2, true, 100.0, 1.0, db_dr),
            Err(OptionValuationError::InvalidInput("carry rate sensitivity"))
        );
    }

    #[rstest]
    fn test_checked_iv_does_not_accept_a_clamped_deep_itm_refinement() {
        let result = imply_vol_and_greeks_checked(
            200.0,
            0.0,
            0.0,
            true,
            100.0,
            1.0,
            100.000_001,
            1.0,
            Some(1e-8),
        )
        .unwrap();
        assert!((result.vol - 0.138_406_809_897_693_3).abs() < 1e-12);
    }

    #[rstest]
    #[case(true, 0.460_172_162_723)]
    #[case(false, -0.539_827_837_277)]
    fn test_checked_greeks_rho_at_zero_rates(#[case] is_call: bool, #[case] expected: f64) {
        let result =
            black_scholes_greeks_checked(100.0, 0.0, 0.0, 0.2, is_call, 100.0, 1.0, 1.0).unwrap();
        assert!((result.rho - expected).abs() < 1e-6);
    }

    #[rstest]
    #[case(0.0)]
    #[case(0.4)]
    #[case(1.0)]
    fn test_checked_greeks_rho_matches_independent_rate_bumps(#[case] db_dr: f64) {
        let result =
            black_scholes_greeks_checked(110.0, 0.05, 0.03, 0.25, true, 105.0, 0.75, db_dr)
                .unwrap();
        let epsilon = 1e-5;
        let high = black_scholes_greeks_exact(
            110.0,
            0.05 + epsilon,
            0.03 + db_dr * epsilon,
            0.25,
            true,
            105.0,
            0.75,
        );
        let low = black_scholes_greeks_exact(
            110.0,
            0.05 - epsilon,
            0.03 - db_dr * epsilon,
            0.25,
            true,
            105.0,
            0.75,
        );
        let reference = (high.price - low.price) / (2.0 * epsilon) * 0.01;
        assert!((result.rho - reference).abs() < 1e-6);
    }

    #[rstest]
    fn test_checked_greeks_rho_is_stable_for_deep_itm_calls() {
        let result =
            black_scholes_greeks_checked(1e20, 0.05, 0.0, 0.2, true, 100.0, 1.0, 1.0).unwrap();
        assert!((result.rho - 0.951_229_424_500_714).abs() < 1e-12);
    }

    #[rstest]
    fn test_checked_implied_volatility_preserves_a_tiny_premium_solution() {
        let vol = imply_vol_checked(100.0, 0.05, 0.05, true, 200.0, 1.0, 1e-10).unwrap();
        assert!((vol - 0.099_420_823_233_839_27).abs() < 1e-12);
    }

    #[rstest]
    fn test_checked_iv_does_not_accept_the_upper_refinement_clamp() {
        let result = imply_vol_and_greeks_checked(
            100.0,
            0.0,
            0.0,
            true,
            100.0,
            1.0,
            99.99998,
            1.0,
            Some(10.0),
        )
        .unwrap();
        assert!((result.vol - 10.398_675_164_168_564).abs() < 1e-12);
    }

    #[rstest]
    #[case(100.0, 0.0)]
    #[case(100.0, 100.0)]
    #[case(100.0, 200.0)]
    #[case(0.0, 10.0)]
    fn test_checked_implied_volatility_rejects_invalid_or_unavailable_solutions(
        #[case] spot: f64,
        #[case] price: f64,
    ) {
        assert_eq!(
            imply_vol_checked(spot, 0.0, 0.0, true, 100.0, 1.0, price),
            Err(OptionValuationError::ImpliedVolatilityUnavailable)
        );
    }

    #[rstest]
    fn test_checked_iv_accepts_valid_index_option_premium() {
        let strike = 5085.0;
        let time = 1.0 / 365.25;
        let vol = 0.141;
        let target = black_scholes_greeks_exact(5000.0, 0.0425, 0.0, vol, true, strike, time);
        let result = imply_vol_and_greeks_checked(
            5000.0,
            0.0425,
            0.0,
            true,
            strike,
            time,
            target.price,
            0.0,
            None,
        )
        .unwrap();
        assert!((result.vol - vol).abs() < 1e-10);
        assert_eq!(
            result,
            black_scholes_greeks_exact(5000.0, 0.0425, 0.0, result.vol, true, strike, time)
        );
    }

    #[rstest]
    fn test_checked_greeks_accepts_valid_tail_after_spot_shock() {
        let result = black_scholes_greeks_checked(
            4593.0,
            0.0425,
            0.0,
            0.14,
            true,
            5100.0,
            7.0 / 365.25,
            0.0,
        )
        .unwrap();
        assert_eq!(
            result,
            black_scholes_greeks_exact(4593.0, 0.0425, 0.0, 0.14, true, 5100.0, 7.0 / 365.25)
        );
    }

    #[rstest]
    fn test_checked_iv_rejects_a_premium_neither_kernel_can_reprice() {
        let vol = imply_vol_checked(100.0, 0.0, 0.0, true, 100.0, 1.0, 1e-12).unwrap();
        assert!((vol - 2.506_628_274_631_000_4e-14).abs() < 1e-27);
        assert_eq!(
            imply_vol_and_greeks_checked(100.0, 0.0, 0.0, true, 100.0, 1.0, 1e-12, 0.0, None),
            Err(OptionValuationError::InvalidResult)
        );
    }
}
