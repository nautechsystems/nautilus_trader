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

use std::{
    fmt,
    ops::{Add, Mul},
};

use implied_vol::{implied_black_volatility, norm_cdf, norm_pdf};
use nautilus_core::{UnixNanos, datetime::unix_nanos_to_iso8601, math::quadratic_interpolation};

use crate::{data::GetTsInit, identifiers::InstrumentId};

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
    let cdf_phi_d1 = norm_cdf(phi * d1);
    let cdf_phi_d2 = norm_cdf(phi * d2);
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

    implied_black_volatility(forward_price, forward, k, t, is_call)
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

impl fmt::Display for GreeksData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl GetTsInit for GreeksData {
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

impl fmt::Display for PortfolioGreeks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl GetTsInit for PortfolioGreeks {
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

impl fmt::Display for YieldCurveData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "InterestRateCurve(curve_name={}, ts_event={}, ts_init={})",
            self.curve_name,
            unix_nanos_to_iso8601(self.ts_event),
            unix_nanos_to_iso8601(self.ts_init)
        )
    }
}

impl GetTsInit for YieldCurveData {
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
