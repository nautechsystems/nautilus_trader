# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.nautilus_pyo3 import black_scholes_greeks
from nautilus_trader.core.nautilus_pyo3 import imply_vol_and_greeks


def test_greeks_accuracy_call():
    s = 100.0
    k = 100.1
    t = 1.0
    r = 0.01
    b = 0.005
    sigma = 0.2
    is_call = True
    eps = 1e-3

    greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0)

    def price0(s):
        return black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0).price

    delta_bnr = (price0(s + eps) - price0(s - eps)) / (2 * eps)
    gamma_bnr = (price0(s + eps) + price0(s - eps) - 2 * price0(s)) / (eps * eps)
    vega_bnr = (
        (
            black_scholes_greeks(s, r, b, sigma + eps, is_call, k, t, 1.0).price
            - black_scholes_greeks(s, r, b, sigma - eps, is_call, k, t, 1.0).price
        )
        / (2 * eps)
        / 100
    )
    theta_bnr = (
        (
            black_scholes_greeks(s, r, b, sigma, is_call, k, t - eps, 1.0).price
            - black_scholes_greeks(s, r, b, sigma, is_call, k, t + eps, 1.0).price
        )
        / (2 * eps)
        / 365.25
    )

    tolerance = 1e-5
    assert abs(greeks.delta - delta_bnr) < tolerance, "Delta difference exceeds tolerance"
    assert abs(greeks.gamma - gamma_bnr) < tolerance, "Gamma difference exceeds tolerance"
    assert abs(greeks.vega - vega_bnr) < tolerance, "Vega difference exceeds tolerance"
    assert abs(greeks.theta - theta_bnr) < tolerance, "Theta difference exceeds tolerance"


def test_greeks_accuracy_put():
    s = 100.0
    k = 100.1
    t = 1.0
    r = 0.01
    b = 0.005
    sigma = 0.2
    is_call = False
    eps = 1e-3

    greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0)

    def price0(s):
        return black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0).price

    delta_bnr = (price0(s + eps) - price0(s - eps)) / (2 * eps)
    gamma_bnr = (price0(s + eps) + price0(s - eps) - 2 * price0(s)) / (eps * eps)
    vega_bnr = (
        (
            black_scholes_greeks(s, r, b, sigma + eps, is_call, k, t, 1.0).price
            - black_scholes_greeks(s, r, b, sigma - eps, is_call, k, t, 1.0).price
        )
        / (2 * eps)
        / 100
    )
    theta_bnr = (
        (
            black_scholes_greeks(s, r, b, sigma, is_call, k, t - eps, 1.0).price
            - black_scholes_greeks(s, r, b, sigma, is_call, k, t + eps, 1.0).price
        )
        / (2 * eps)
        / 365.25
    )

    tolerance = 1e-5
    assert abs(greeks.delta - delta_bnr) < tolerance, "Delta difference exceeds tolerance"
    assert abs(greeks.gamma - gamma_bnr) < tolerance, "Gamma difference exceeds tolerance"
    assert abs(greeks.vega - vega_bnr) < tolerance, "Vega difference exceeds tolerance"
    assert abs(greeks.theta - theta_bnr) < tolerance, "Theta difference exceeds tolerance"


def test_imply_vol_and_greeks_accuracy_call():
    s = 100.0
    k = 100.1
    t = 1.0
    r = 0.01
    b = 0.005
    sigma = 0.2
    is_call = True

    base_greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0)
    price = base_greeks.price

    implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price, 1.0)

    tolerance = 1e-5
    assert abs(implied_result.vol - sigma) < tolerance, "Vol difference exceeds tolerance"
    assert (
        abs(implied_result.price - base_greeks.price) < tolerance
    ), "Price difference exceeds tolerance"
    assert (
        abs(implied_result.delta - base_greeks.delta) < tolerance
    ), "Delta difference exceeds tolerance"
    assert (
        abs(implied_result.gamma - base_greeks.gamma) < tolerance
    ), "Gamma difference exceeds tolerance"
    assert (
        abs(implied_result.vega - base_greeks.vega) < tolerance
    ), "Vega difference exceeds tolerance"
    assert (
        abs(implied_result.theta - base_greeks.theta) < tolerance
    ), "Theta difference exceeds tolerance"


def test_imply_vol_and_greeks_accuracy_put():
    s = 100.0
    k = 100.1
    t = 1.0
    r = 0.01
    b = 0.005
    sigma = 0.2
    is_call = False

    base_greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.0)
    price = base_greeks.price

    implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, price, 1.0)

    tolerance = 1e-5
    assert abs(implied_result.vol - sigma) < tolerance, "Vol difference exceeds tolerance"
    assert (
        abs(implied_result.price - base_greeks.price) < tolerance
    ), "Price difference exceeds tolerance"
    assert (
        abs(implied_result.delta - base_greeks.delta) < tolerance
    ), "Delta difference exceeds tolerance"
    assert (
        abs(implied_result.gamma - base_greeks.gamma) < tolerance
    ), "Gamma difference exceeds tolerance"
    assert (
        abs(implied_result.vega - base_greeks.vega) < tolerance
    ), "Vega difference exceeds tolerance"
    assert (
        abs(implied_result.theta - base_greeks.theta) < tolerance
    ), "Theta difference exceeds tolerance"
