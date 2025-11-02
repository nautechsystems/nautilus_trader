// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    fmt::Display,
    ops::{Add, Mul},
};

use implied_vol::{DefaultSpecialFn, ImpliedBlackVolatility, SpecialFn};
use nautilus_core::{UnixNanos, datetime::unix_nanos_to_iso8601, math::quadratic_interpolation};

use crate::{data::HasTsInit, identifiers::InstrumentId};

const FRAC_SQRT_2_PI: f64 = f64::from_bits(0x3fd9884533d43651);

#[inline(always)]
fn norm_pdf(x: f64) -> f64 {
    FRAC_SQRT_2_PI * (-0.5 * x * x).exp()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BlackScholesGreeksResult {
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
}

// dS_t = S_t * (b * dt + sigma * dW_t) (stock)
// dC_t = r * C_t * dt (cash numeraire)
#[allow(clippy::too_many_arguments)]
pub fn black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    sigma: f64,
    is_call: bool,
    k: f64,
    t: f64,
    multiplier: f64,
) -> BlackScholesGreeksResult {
    let phi = if is_call { 1.0 } else { -1.0 };
    let scaled_vol = sigma * t.sqrt();
    let d1 = ((s / k).ln() + (b + 0.5 * sigma.powi(2)) * t) / scaled_vol;
    let d2 = d1 - scaled_vol;
    let cdf_phi_d1 = DefaultSpecialFn::norm_cdf(phi * d1);
    let cdf_phi_d2 = DefaultSpecialFn::norm_cdf(phi * d2);
    let dist_d1 = norm_pdf(d1);
    let df = ((b - r) * t).exp();
    let s_t = s * df;
    let k_t = k * (-r * t).exp();

    let price = multiplier * phi * (s_t * cdf_phi_d1 - k_t * cdf_phi_d2);
    let delta = multiplier * phi * df * cdf_phi_d1;
    let gamma = multiplier * df * dist_d1 / (s * scaled_vol);
    let vega = multiplier * s_t * t.sqrt() * dist_d1 * 0.01; // in absolute percent change
    let theta = multiplier
        * (s_t * (-dist_d1 * sigma / (2.0 * t.sqrt()) - phi * (b - r) * cdf_phi_d1)
            - phi * r * k_t * cdf_phi_d2)
        * 0.0027378507871321013; // 1 / 365.25 in change per calendar day

    BlackScholesGreeksResult {
        price,
        delta,
        gamma,
        vega,
        theta,
    }
}

pub fn imply_vol(s: f64, r: f64, b: f64, is_call: bool, k: f64, t: f64, price: f64) -> f64 {
    let forward = s * b.exp();
    let forward_price = price * (r * t).exp();

    ImpliedBlackVolatility::builder()
        .option_price(forward_price)
        .forward(forward)
        .strike(k)
        .expiry(t)
        .is_call(is_call)
        .build_unchecked()
        .calculate::<DefaultSpecialFn>()
        .unwrap_or(0.0)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct ImplyVolAndGreeksResult {
    pub vol: f64,
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
    multiplier: f64,
) -> ImplyVolAndGreeksResult {
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    let greeks = black_scholes_greeks(s, r, b, vol, is_call, k, t, multiplier);

    ImplyVolAndGreeksResult {
        vol,
        price: greeks.price,
        delta: greeks.delta,
        gamma: greeks.gamma,
        vega: greeks.vega,
        theta: greeks.theta,
    }
}

#[derive(Debug, Clone)]
pub struct GreeksData {
    pub ts_init: UnixNanos,
    pub ts_event: UnixNanos,
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
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    // in the money probability, P(phi * S_T > phi * K), phi = 1 if is_call else -1
    pub itm_prob: f64,
}

impl GreeksData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ts_init: UnixNanos,
        ts_event: UnixNanos,
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
        delta: f64,
        gamma: f64,
        vega: f64,
        theta: f64,
        itm_prob: f64,
    ) -> Self {
        Self {
            ts_init,
            ts_event,
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
            delta,
            gamma,
            vega,
            theta,
            itm_prob,
        }
    }

    pub fn from_delta(
        instrument_id: InstrumentId,
        delta: f64,
        multiplier: f64,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            ts_init: ts_event,
            ts_event,
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
            delta,
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
            itm_prob: 0.0,
        }
    }
}

impl Default for GreeksData {
    fn default() -> Self {
        Self {
            ts_init: UnixNanos::default(),
            ts_event: UnixNanos::default(),
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
            delta: 0.0,
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
            itm_prob: 0.0,
        }
    }
}

impl Display for GreeksData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GreeksData(instrument_id={}, expiry={}, itm_prob={:.2}%, vol={:.2}%, pnl={:.2}, price={:.2}, delta={:.2}, gamma={:.2}, vega={:.2}, theta={:.2}, quantity={}, ts_init={})",
            self.instrument_id,
            self.expiry,
            self.itm_prob * 100.0,
            self.vol * 100.0,
            self.pnl,
            self.price,
            self.delta,
            self.gamma,
            self.vega,
            self.theta,
            self.quantity,
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

// Implement multiplication for quantity * greeks
impl Mul<&GreeksData> for f64 {
    type Output = GreeksData;

    fn mul(self, greeks: &GreeksData) -> GreeksData {
        GreeksData {
            ts_init: greeks.ts_init,
            ts_event: greeks.ts_event,
            instrument_id: greeks.instrument_id,
            is_call: greeks.is_call,
            strike: greeks.strike,
            expiry: greeks.expiry,
            expiry_in_days: greeks.expiry_in_days,
            expiry_in_years: greeks.expiry_in_years,
            multiplier: greeks.multiplier,
            quantity: greeks.quantity,
            underlying_price: greeks.underlying_price,
            interest_rate: greeks.interest_rate,
            cost_of_carry: greeks.cost_of_carry,
            vol: greeks.vol,
            pnl: self * greeks.pnl,
            price: self * greeks.price,
            delta: self * greeks.delta,
            gamma: self * greeks.gamma,
            vega: self * greeks.vega,
            theta: self * greeks.theta,
            itm_prob: greeks.itm_prob,
        }
    }
}

impl HasTsInit for GreeksData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[derive(Debug, Clone)]
pub struct PortfolioGreeks {
    pub ts_init: UnixNanos,
    pub ts_event: UnixNanos,
    pub pnl: f64,
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
}

impl PortfolioGreeks {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ts_init: UnixNanos,
        ts_event: UnixNanos,
        pnl: f64,
        price: f64,
        delta: f64,
        gamma: f64,
        vega: f64,
        theta: f64,
    ) -> Self {
        Self {
            ts_init,
            ts_event,
            pnl,
            price,
            delta,
            gamma,
            vega,
            theta,
        }
    }
}

impl Default for PortfolioGreeks {
    fn default() -> Self {
        Self {
            ts_init: UnixNanos::default(),
            ts_event: UnixNanos::default(),
            pnl: 0.0,
            price: 0.0,
            delta: 0.0,
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
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
            self.delta,
            self.gamma,
            self.vega,
            self.theta,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

impl Add for PortfolioGreeks {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            ts_init: self.ts_init,
            ts_event: self.ts_event,
            pnl: self.pnl + other.pnl,
            price: self.price + other.price,
            delta: self.delta + other.delta,
            gamma: self.gamma + other.gamma,
            vega: self.vega + other.vega,
            theta: self.theta + other.theta,
        }
    }
}

impl From<GreeksData> for PortfolioGreeks {
    fn from(greeks: GreeksData) -> Self {
        Self {
            ts_init: greeks.ts_init,
            ts_event: greeks.ts_event,
            pnl: greeks.pnl,
            price: greeks.price,
            delta: greeks.delta,
            gamma: greeks.gamma,
            vega: greeks.vega,
            theta: greeks.theta,
        }
    }
}

impl HasTsInit for PortfolioGreeks {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[derive(Debug, Clone)]
pub struct YieldCurveData {
    pub ts_init: UnixNanos,
    pub ts_event: UnixNanos,
    pub curve_name: String,
    pub tenors: Vec<f64>,
    pub interest_rates: Vec<f64>,
}

impl YieldCurveData {
    pub fn new(
        ts_init: UnixNanos,
        ts_event: UnixNanos,
        curve_name: String,
        tenors: Vec<f64>,
        interest_rates: Vec<f64>,
    ) -> Self {
        Self {
            ts_init,
            ts_event,
            curve_name,
            tenors,
            interest_rates,
        }
    }

    // Interpolate the yield curve for a given expiry time
    pub fn get_rate(&self, expiry_in_years: f64) -> f64 {
        if self.interest_rates.len() == 1 {
            return self.interest_rates[0];
        }

        quadratic_interpolation(expiry_in_years, &self.tenors, &self.interest_rates)
    }
}

impl Display for YieldCurveData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "InterestRateCurve(curve_name={}, ts_event={}, ts_init={})",
            self.curve_name,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

impl HasTsInit for YieldCurveData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Default for YieldCurveData {
    fn default() -> Self {
        Self {
            ts_init: UnixNanos::default(),
            ts_event: UnixNanos::default(),
            curve_name: "USD".to_string(),
            tenors: vec![0.5, 1.0, 1.5, 2.0, 2.5],
            interest_rates: vec![0.04, 0.04, 0.04, 0.04, 0.04],
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
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_500_000_000),
            InstrumentId::from("SPY240315C00500000.OPRA"),
            true,
            500.0,
            20240315,
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
            0.65,
            0.003,
            15.2,
            -0.08,
            0.75,
        )
    }

    fn create_test_portfolio_greeks() -> PortfolioGreeks {
        PortfolioGreeks::new(
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_500_000_000),
            1500.0,
            125.5,
            2.15,
            0.008,
            42.7,
            -2.3,
        )
    }

    fn create_test_yield_curve() -> YieldCurveData {
        YieldCurveData::new(
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_500_000_000),
            "USD".to_string(),
            vec![0.25, 0.5, 1.0, 2.0, 5.0],
            vec![0.025, 0.03, 0.035, 0.04, 0.045],
        )
    }

    #[rstest]
    fn test_black_scholes_greeks_result_creation() {
        let result = BlackScholesGreeksResult {
            price: 25.5,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
        };

        assert_eq!(result.price, 25.5);
        assert_eq!(result.delta, 0.65);
        assert_eq!(result.gamma, 0.003);
        assert_eq!(result.vega, 15.2);
        assert_eq!(result.theta, -0.08);
    }

    #[rstest]
    fn test_black_scholes_greeks_result_clone_and_copy() {
        let result1 = BlackScholesGreeksResult {
            price: 25.5,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
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
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
        };
        let debug_str = format!("{result:?}");

        assert!(debug_str.contains("BlackScholesGreeksResult"));
        assert!(debug_str.contains("25.5"));
        assert!(debug_str.contains("0.65"));
    }

    #[rstest]
    fn test_imply_vol_and_greeks_result_creation() {
        let result = ImplyVolAndGreeksResult {
            vol: 0.2,
            price: 25.5,
            delta: 0.65,
            gamma: 0.003,
            vega: 15.2,
            theta: -0.08,
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
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

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
        let sigma = 0.2;
        let is_call = false;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

        assert!(greeks.price > 0.0);
        assert!(greeks.delta < 0.0 && greeks.delta > -1.0);
        assert!(greeks.gamma > 0.0);
        assert!(greeks.vega > 0.0);
        assert!(greeks.theta < 0.0); // Time decay for long option
    }

    #[rstest]
    fn test_black_scholes_greeks_with_multiplier() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 100.0;

        let greeks_1x = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0);
        let greeks_100x = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

        let tolerance = 1e-10;
        assert!((greeks_100x.price - greeks_1x.price * 100.0).abs() < tolerance);
        assert!((greeks_100x.delta - greeks_1x.delta * 100.0).abs() < tolerance);
        assert!((greeks_100x.gamma - greeks_1x.gamma * 100.0).abs() < tolerance);
        assert!((greeks_100x.vega - greeks_1x.vega * 100.0).abs() < tolerance);
        assert!((greeks_100x.theta - greeks_1x.theta * 100.0).abs() < tolerance);
    }

    #[rstest]
    fn test_black_scholes_greeks_deep_itm_call() {
        let s = 150.0;
        let r = 0.05;
        let b = 0.05;
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

        assert!(greeks.delta > 0.9); // Deep ITM call has delta close to 1
        assert!(greeks.gamma > 0.0 && greeks.gamma < 0.01); // Low gamma for deep ITM
    }

    #[rstest]
    fn test_black_scholes_greeks_deep_otm_call() {
        let s = 50.0;
        let r = 0.05;
        let b = 0.05;
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

        assert!(greeks.delta < 0.1); // Deep OTM call has delta close to 0
        assert!(greeks.gamma > 0.0 && greeks.gamma < 0.01); // Low gamma for deep OTM
    }

    #[rstest]
    fn test_black_scholes_greeks_zero_time() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 0.0001; // Near zero time
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

        assert!(greeks.price >= 0.0);
        assert!(greeks.theta.is_finite());
    }

    #[rstest]
    fn test_imply_vol_basic() {
        let s = 100.0;
        let r = 0.05;
        let b = 0.05;
        let sigma = 0.2;
        let is_call = true;
        let k = 100.0;
        let t = 1.0;

        let theoretical_price = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0).price;
        let implied_vol = imply_vol(s, r, b, is_call, k, t, theoretical_price);

        let tolerance = 1e-6;
        assert!((implied_vol - sigma).abs() < tolerance);
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
        assert_eq!(greeks.expiry, 20240315);
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

        assert!(display_str.contains("GreeksData"));
        assert!(display_str.contains("SPY240315C00500000.OPRA"));
        assert!(display_str.contains("20240315"));
        assert!(display_str.contains("75.00%")); // itm_prob * 100
        assert!(display_str.contains("20.00%")); // vol * 100
        assert!(display_str.contains("250.00")); // pnl
        assert!(display_str.contains("25.50")); // price
        assert!(display_str.contains("0.65")); // delta
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

        assert!(display_str.contains("PortfolioGreeks"));
        assert!(display_str.contains("1500.00")); // pnl
        assert!(display_str.contains("125.50")); // price
        assert!(display_str.contains("2.15")); // delta
        assert!(display_str.contains("0.01")); // gamma (rounded)
        assert!(display_str.contains("42.70")); // vega
        assert!(display_str.contains("-2.30")); // theta
    }

    #[rstest]
    fn test_portfolio_greeks_addition() {
        let greeks1 = PortfolioGreeks::new(
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_500_000_000),
            100.0,
            50.0,
            1.0,
            0.005,
            20.0,
            -1.0,
        );
        let greeks2 = PortfolioGreeks::new(
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(2_500_000_000),
            200.0,
            75.0,
            1.5,
            0.003,
            25.0,
            -1.5,
        );

        let result = greeks1 + greeks2;

        assert_eq!(result.ts_init, UnixNanos::from(1_000_000_000)); // Uses first ts_init
        assert_eq!(result.ts_event, UnixNanos::from(1_500_000_000)); // Uses first ts_event
        assert_eq!(result.pnl, 300.0);
        assert_eq!(result.price, 125.0);
        assert_eq!(result.delta, 2.5);
        assert_eq!(result.gamma, 0.008);
        assert_eq!(result.vega, 45.0);
        assert_eq!(result.theta, -2.5);
    }

    #[rstest]
    fn test_portfolio_greeks_from_greeks_data() {
        let greeks_data = create_test_greeks_data();
        let portfolio_greeks: PortfolioGreeks = greeks_data.clone().into();

        assert_eq!(portfolio_greeks.ts_init, greeks_data.ts_init);
        assert_eq!(portfolio_greeks.ts_event, greeks_data.ts_event);
        assert_eq!(portfolio_greeks.pnl, greeks_data.pnl);
        assert_eq!(portfolio_greeks.price, greeks_data.price);
        assert_eq!(portfolio_greeks.delta, greeks_data.delta);
        assert_eq!(portfolio_greeks.gamma, greeks_data.gamma);
        assert_eq!(portfolio_greeks.vega, greeks_data.vega);
        assert_eq!(portfolio_greeks.theta, greeks_data.theta);
    }

    #[rstest]
    fn test_portfolio_greeks_has_ts_init() {
        let portfolio_greeks = create_test_portfolio_greeks();
        assert_eq!(portfolio_greeks.ts_init(), UnixNanos::from(1_000_000_000));
    }

    #[rstest]
    fn test_yield_curve_data_new() {
        let curve = create_test_yield_curve();

        assert_eq!(curve.ts_init, UnixNanos::from(1_000_000_000));
        assert_eq!(curve.ts_event, UnixNanos::from(1_500_000_000));
        assert_eq!(curve.curve_name, "USD");
        assert_eq!(curve.tenors, vec![0.25, 0.5, 1.0, 2.0, 5.0]);
        assert_eq!(curve.interest_rates, vec![0.025, 0.03, 0.035, 0.04, 0.045]);
    }

    #[rstest]
    fn test_yield_curve_data_default() {
        let curve = YieldCurveData::default();

        assert_eq!(curve.ts_init, UnixNanos::default());
        assert_eq!(curve.ts_event, UnixNanos::default());
        assert_eq!(curve.curve_name, "USD");
        assert_eq!(curve.tenors, vec![0.5, 1.0, 1.5, 2.0, 2.5]);
        assert_eq!(curve.interest_rates, vec![0.04, 0.04, 0.04, 0.04, 0.04]);
    }

    #[rstest]
    fn test_yield_curve_data_get_rate_single_point() {
        let curve = YieldCurveData::new(
            UnixNanos::default(),
            UnixNanos::default(),
            "USD".to_string(),
            vec![1.0],
            vec![0.05],
        );

        assert_eq!(curve.get_rate(0.5), 0.05);
        assert_eq!(curve.get_rate(1.0), 0.05);
        assert_eq!(curve.get_rate(2.0), 0.05);
    }

    #[rstest]
    fn test_yield_curve_data_get_rate_interpolation() {
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
    fn test_yield_curve_data_display() {
        let curve = create_test_yield_curve();
        let display_str = format!("{curve}");

        assert!(display_str.contains("InterestRateCurve"));
        assert!(display_str.contains("USD"));
    }

    #[rstest]
    fn test_yield_curve_data_has_ts_init() {
        let curve = create_test_yield_curve();
        assert_eq!(curve.ts_init(), UnixNanos::from(1_000_000_000));
    }

    #[rstest]
    fn test_yield_curve_data_clone() {
        let curve1 = create_test_yield_curve();
        let curve2 = curve1.clone();

        assert_eq!(curve1.curve_name, curve2.curve_name);
        assert_eq!(curve1.tenors, curve2.tenors);
        assert_eq!(curve1.interest_rates, curve2.interest_rates);
    }

    #[rstest]
    fn test_black_scholes_greeks_extreme_values() {
        let s = 1000.0;
        let r = 0.1;
        let b = 0.1;
        let sigma = 0.5;
        let is_call = true;
        let k = 10.0; // Very deep ITM
        let t = 0.1;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

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
        let sigma = 2.0; // 200% volatility
        let is_call = true;
        let k = 100.0;
        let t = 1.0;
        let multiplier = 1.0;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, multiplier);

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
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_500_000_000),
            InstrumentId::from("SPY240315P00480000.OPRA"),
            false, // Put option
            480.0,
            20240315,
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
            -0.35, // Negative delta for put
            0.002,
            12.8,
            -0.06,
            0.25,
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
        let sigma = 0.2;
        let is_call = true;
        let eps = 1e-3;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0);

        let price0 = |s: f64| black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0).price;

        let delta_bnr = (price0(s + eps) - price0(s - eps)) / (2.0 * eps);
        let gamma_bnr = (price0(s + eps) + price0(s - eps) - 2.0 * price0(s)) / (eps * eps);
        let vega_bnr = (black_scholes_greeks(s, r, b, sigma + eps, is_call, k, t, 1.0).price
            - black_scholes_greeks(s, r, b, sigma - eps, is_call, k, t, 1.0).price)
            / (2.0 * eps)
            / 100.0;
        let theta_bnr = (black_scholes_greeks(s, r, b, sigma, is_call, k, t - eps, 1.0).price
            - black_scholes_greeks(s, r, b, sigma, is_call, k, t + eps, 1.0).price)
            / (2.0 * eps)
            / 365.25;

        let tolerance = 1e-5;
        assert!(
            (greeks.delta - delta_bnr).abs() < tolerance,
            "Delta difference exceeds tolerance"
        );
        assert!(
            (greeks.gamma - gamma_bnr).abs() < tolerance,
            "Gamma difference exceeds tolerance"
        );
        assert!(
            (greeks.vega - vega_bnr).abs() < tolerance,
            "Vega difference exceeds tolerance"
        );
        assert!(
            (greeks.theta - theta_bnr).abs() < tolerance,
            "Theta difference exceeds tolerance"
        );
    }

    #[rstest]
    fn test_greeks_accuracy_put() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let sigma = 0.2;
        let is_call = false;
        let eps = 1e-3;

        let greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0);

        let price0 = |s: f64| black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0).price;

        let delta_bnr = (price0(s + eps) - price0(s - eps)) / (2.0 * eps);
        let gamma_bnr = (price0(s + eps) + price0(s - eps) - 2.0 * price0(s)) / (eps * eps);
        let vega_bnr = (black_scholes_greeks(s, r, b, sigma + eps, is_call, k, t, 1.0).price
            - black_scholes_greeks(s, r, b, sigma - eps, is_call, k, t, 1.0).price)
            / (2.0 * eps)
            / 100.0;
        let theta_bnr = (black_scholes_greeks(s, r, b, sigma, is_call, k, t - eps, 1.0).price
            - black_scholes_greeks(s, r, b, sigma, is_call, k, t + eps, 1.0).price)
            / (2.0 * eps)
            / 365.25;

        let tolerance = 1e-5;
        assert!(
            (greeks.delta - delta_bnr).abs() < tolerance,
            "Delta difference exceeds tolerance"
        );
        assert!(
            (greeks.gamma - gamma_bnr).abs() < tolerance,
            "Gamma difference exceeds tolerance"
        );
        assert!(
            (greeks.vega - vega_bnr).abs() < tolerance,
            "Vega difference exceeds tolerance"
        );
        assert!(
            (greeks.theta - theta_bnr).abs() < tolerance,
            "Theta difference exceeds tolerance"
        );
    }

    #[rstest]
    fn test_imply_vol_and_greeks_accuracy_call() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let sigma = 0.2;
        let is_call = true;

        let base_greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0);
        let price = base_greeks.price;

        let implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price, 1.0);

        let tolerance = 1e-5;
        assert!(
            (implied_result.vol - sigma).abs() < tolerance,
            "Vol difference exceeds tolerance"
        );
        assert!(
            (implied_result.price - base_greeks.price).abs() < tolerance,
            "Price difference exceeds tolerance"
        );
        assert!(
            (implied_result.delta - base_greeks.delta).abs() < tolerance,
            "Delta difference exceeds tolerance"
        );
        assert!(
            (implied_result.gamma - base_greeks.gamma).abs() < tolerance,
            "Gamma difference exceeds tolerance"
        );
        assert!(
            (implied_result.vega - base_greeks.vega).abs() < tolerance,
            "Vega difference exceeds tolerance"
        );
        assert!(
            (implied_result.theta - base_greeks.theta).abs() < tolerance,
            "Theta difference exceeds tolerance"
        );
    }

    #[rstest]
    fn test_imply_vol_and_greeks_accuracy_put() {
        let s = 100.0;
        let k = 100.1;
        let t = 1.0;
        let r = 0.01;
        let b = 0.005;
        let sigma = 0.2;
        let is_call = false;

        let base_greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0);
        let price = base_greeks.price;

        let implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price, 1.0);

        let tolerance = 1e-5;
        assert!(
            (implied_result.vol - sigma).abs() < tolerance,
            "Vol difference exceeds tolerance"
        );
        assert!(
            (implied_result.price - base_greeks.price).abs() < tolerance,
            "Price difference exceeds tolerance"
        );
        assert!(
            (implied_result.delta - base_greeks.delta).abs() < tolerance,
            "Delta difference exceeds tolerance"
        );
        assert!(
            (implied_result.gamma - base_greeks.gamma).abs() < tolerance,
            "Gamma difference exceeds tolerance"
        );
        assert!(
            (implied_result.vega - base_greeks.vega).abs() < tolerance,
            "Vega difference exceeds tolerance"
        );
        assert!(
            (implied_result.theta - base_greeks.theta).abs() < tolerance,
            "Theta difference exceeds tolerance"
        );
    }
}
