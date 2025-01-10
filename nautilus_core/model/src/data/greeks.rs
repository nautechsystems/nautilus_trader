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

use implied_vol::{implied_black_volatility, norm_cdf, norm_pdf};

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
