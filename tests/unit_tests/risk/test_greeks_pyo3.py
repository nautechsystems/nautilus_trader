# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.nautilus_pyo3 import refine_vol_and_greeks


class TestBlackScholesGreeksPyO3:
    def test_black_scholes_greeks_basic_call(self):
        # Test basic call option with exact expected values
        result = black_scholes_greeks(
            s=100.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        # Test that vol field is accessible (new field added in previous commit)
        assert result.vol == 0.2, "vol field should be accessible and match input"

        # Exact expected values (tolerance 1e-5 for f64 precision)
        tol = 1e-5
        assert abs(result.price - 10.4505767822) < tol, (
            f"Price mismatch: {result.price} vs 10.4505767822"
        )
        assert abs(result.delta - 0.6368305683) < tol, (
            f"Delta mismatch: {result.delta} vs 0.6368305683"
        )
        assert abs(result.gamma - 0.0187620167) < tol, (
            f"Gamma mismatch: {result.gamma} vs 0.0187620167"
        )
        assert abs(result.vega - 0.3752403641) < tol, (
            f"Vega mismatch: {result.vega} vs 0.3752403641"
        )
        assert abs(result.theta - (-0.0175606508)) < tol, (
            f"Theta mismatch: {result.theta} vs -0.0175606508"
        )

    def test_black_scholes_greeks_target_price_refinement(self):
        # Test that refine_vol_and_greeks refines the vol to match the target price
        s = 100.0
        r = 0.05
        b = 0.05
        initial_vol = 0.2
        is_call = True
        k = 100.0
        t = 1.0
        multiplier = 1.0

        # Calculate the price with the initial vol
        initial_result = black_scholes_greeks(
            s,
            r,
            b,
            initial_vol,
            is_call,
            k,
            t,
            multiplier,
        )
        target_price = initial_result.price

        # Now use a slightly different vol and refine it using refine_vol_and_greeks
        refined_vol = initial_vol * 1.1  # 10% higher vol
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            refined_vol,
            multiplier,
        )

        # Exact expected values (tolerance 1e-3 for refinement precision)
        tol = 1e-3
        assert abs(refined_result.price - target_price) < tol, (
            f"Refined price should match target: {refined_result.price} vs {target_price}"
        )
        # Vol should converge to the initial vol (0.2) that produced the target price
        assert abs(refined_result.vol - initial_vol) < 1e-3, (
            f"Refined vol should converge to initial vol: {refined_result.vol} vs {initial_vol}"
        )

    def test_black_scholes_greeks_target_price_refinement_put(self):
        # Test refine_vol_and_greeks for put option
        s = 100.0
        r = 0.05
        b = 0.05
        initial_vol = 0.25
        is_call = False
        k = 105.0
        t = 0.5
        multiplier = 1.0

        # Calculate the price with the initial vol
        initial_result = black_scholes_greeks(
            s,
            r,
            b,
            initial_vol,
            is_call,
            k,
            t,
            multiplier,
        )
        target_price = initial_result.price

        # Now use a different vol and refine it using refine_vol_and_greeks
        refined_vol = initial_vol * 0.8  # 20% lower vol
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            refined_vol,
            multiplier,
        )

        # Exact expected values (tolerance 1.5e-3 for refinement precision, slightly relaxed for put)
        tol = 1.5e-3
        assert abs(refined_result.price - target_price) < tol, (
            f"Refined price should match target: {refined_result.price} vs {target_price}"
        )
        # Vol should converge to the initial vol (0.25) that produced the target price
        assert abs(refined_result.vol - initial_vol) < 1e-3, (
            f"Refined vol should converge to initial vol: {refined_result.vol} vs {initial_vol}"
        )

    def test_black_scholes_greeks_target_price_convergence(self):
        # Test that refine_vol_and_greeks converges within tolerance
        s = 100.0
        r = 0.05
        b = 0.05
        initial_vol = 0.2
        is_call = True
        k = 100.0
        t = 1.0
        multiplier = 1.0

        # Calculate the price with the initial vol
        initial_result = black_scholes_greeks(
            s,
            r,
            b,
            initial_vol,
            is_call,
            k,
            t,
            multiplier,
        )
        target_price = initial_result.price

        # Use a significantly different vol to test convergence
        refined_vol = initial_vol * 1.5  # 50% higher vol
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            refined_vol,
            multiplier,
        )

        # Exact expected values (tolerance 2e-2 for convergence with larger initial error)
        tol = 2e-2
        assert abs(refined_result.price - target_price) < tol, (
            f"Refined price should converge to target: {refined_result.price} vs {target_price}"
        )
        # Vol should converge to the initial vol (0.2) that produced the target price
        assert abs(refined_result.vol - initial_vol) < 1e-2, (
            f"Refined vol should converge to initial vol: {refined_result.vol} vs {initial_vol}"
        )

    def test_black_scholes_greeks_result_properties(self):
        # Test that BlackScholesGreeksResult has all expected properties
        result = black_scholes_greeks(
            s=100.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        # Test all properties are accessible
        assert hasattr(result, "price")
        assert hasattr(result, "vol")
        assert hasattr(result, "delta")
        assert hasattr(result, "gamma")
        assert hasattr(result, "vega")
        assert hasattr(result, "theta")

        # Test all values match expected values exactly
        tol = 1e-5
        assert abs(result.price - 10.4505767822) < tol
        assert abs(result.vol - 0.2) < tol
        assert abs(result.delta - 0.6368305683) < tol
        assert abs(result.gamma - 0.0187620167) < tol
        assert abs(result.vega - 0.3752403641) < tol
        assert abs(result.theta - (-0.0175606508)) < tol

    def test_black_scholes_greeks_put_option(self):
        # Test put option with exact expected values
        result = black_scholes_greeks(
            s=100.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=False,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        # Exact expected values (tolerance 1e-5 for f64 precision)
        tol = 1e-5
        assert abs(result.price - 5.5735168457) < tol, (
            f"Price mismatch: {result.price} vs 5.5735168457"
        )
        assert abs(result.delta - (-0.3631694317)) < tol, (
            f"Delta mismatch: {result.delta} vs -0.3631694317"
        )
        assert abs(result.gamma - 0.0187620167) < tol, (
            f"Gamma mismatch: {result.gamma} vs 0.0187620167"
        )
        assert abs(result.vega - 0.3752403641) < tol, (
            f"Vega mismatch: {result.vega} vs 0.3752403641"
        )
        assert abs(result.theta - (-0.0045390302)) < tol, (
            f"Theta mismatch: {result.theta} vs -0.0045390302"
        )

    def test_black_scholes_greeks_itm_call(self):
        # Test in-the-money call option with exact expected values
        result = black_scholes_greeks(
            s=110.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        assert result.vol == 0.2
        # Exact expected values (tolerance 1e-5 for f64 precision)
        tol = 1e-5
        assert abs(result.price - 17.6629486084) < tol, (
            f"Price mismatch: {result.price} vs 17.6629486084"
        )
        assert abs(result.delta - 0.7957542539) < tol, (
            f"Delta mismatch: {result.delta} vs 0.7957542539"
        )

    def test_black_scholes_greeks_otm_call(self):
        # Test out-of-the-money call option with exact expected values
        result = black_scholes_greeks(
            s=90.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        assert result.vol == 0.2
        # Exact expected values (tolerance 1e-5 for f64 precision)
        tol = 1e-5
        assert abs(result.price - 5.0912132263) < tol, (
            f"Price mismatch: {result.price} vs 5.0912132263"
        )
        assert abs(result.delta - 0.4298316240) < tol, (
            f"Delta mismatch: {result.delta} vs 0.4298316240"
        )

    def test_black_scholes_greeks_multiplier(self):
        # Test multiplier affects all greeks appropriately
        multiplier = 100.0
        result = black_scholes_greeks(
            s=100.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=multiplier,
        )

        # Compare with unit multiplier
        result_unit = black_scholes_greeks(
            s=100.0,
            r=0.05,
            b=0.05,
            vol=0.2,
            is_call=True,
            k=100.0,
            t=1.0,
            multiplier=1.0,
        )

        assert abs(result.price - result_unit.price * multiplier) < 1e-6
        assert abs(result.delta - result_unit.delta * multiplier) < 1e-6
        assert abs(result.gamma - result_unit.gamma * multiplier) < 1e-6
        assert abs(result.vega - result_unit.vega * multiplier) < 1e-6
        assert abs(result.theta - result_unit.theta * multiplier) < 1e-6
        assert result.vol == result_unit.vol  # Vol should not change with multiplier

    def test_imply_vol_and_greeks_call(self):
        # Test imply_vol_and_greeks for call option
        s = 100.0
        r = 0.05
        b = 0.05
        true_vol = 0.2
        is_call = True
        k = 100.0
        t = 1.0
        multiplier = 1.0

        # Calculate the true price using known volatility
        true_result = black_scholes_greeks(s, r, b, true_vol, is_call, k, t, multiplier)
        market_price = true_result.price

        # Now imply vol from market price
        implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, market_price, multiplier)

        # Exact expected values (tolerance 1e-3 for implied vol precision)
        tol = 1e-3
        # Vol should match the true vol (0.2) that was used to generate the market price
        assert abs(implied_result.vol - true_vol) < tol, (
            f"Implied vol should match true vol: {implied_result.vol} vs {true_vol}"
        )
        assert abs(implied_result.price - market_price) < tol, (
            f"Implied price should match market price: {implied_result.price} vs {market_price}"
        )
        # Greeks should match true greeks (tolerance 1e-2 for implied vol precision)
        assert abs(implied_result.delta - true_result.delta) < 1e-2, (
            f"Implied delta mismatch: {implied_result.delta} vs {true_result.delta}"
        )
        assert abs(implied_result.gamma - true_result.gamma) < 1e-2, (
            f"Implied gamma mismatch: {implied_result.gamma} vs {true_result.gamma}"
        )

    def test_imply_vol_and_greeks_put(self):
        # Test imply_vol_and_greeks for put option
        s = 100.0
        r = 0.05
        b = 0.05
        true_vol = 0.25
        is_call = False
        k = 105.0
        t = 0.5
        multiplier = 1.0

        # Calculate the true price using known volatility
        true_result = black_scholes_greeks(s, r, b, true_vol, is_call, k, t, multiplier)
        market_price = true_result.price

        # Now imply vol from market price
        implied_result = imply_vol_and_greeks(s, r, b, is_call, k, t, market_price, multiplier)

        # Exact expected values (tolerance 1e-2 for implied vol precision)
        tol = 1e-2
        # Vol should match the true vol (0.25) that was used to generate the market price
        assert abs(implied_result.vol - true_vol) < tol, (
            f"Implied vol should match true vol: {implied_result.vol} vs {true_vol}"
        )
        assert abs(implied_result.price - market_price) < tol, (
            f"Implied price should match market price: {implied_result.price} vs {market_price}"
        )

    def test_refine_vol_and_greeks_exact_match(self):
        # Test refine_vol_and_greeks when initial guess is correct
        s = 100.0
        r = 0.05
        b = 0.05
        true_vol = 0.2
        is_call = True
        k = 100.0
        t = 1.0
        multiplier = 1.0

        # Calculate the target price
        true_result = black_scholes_greeks(s, r, b, true_vol, is_call, k, t, multiplier)
        target_price = true_result.price

        # Refine using the true vol as initial guess
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            true_vol,
            multiplier,
        )

        # Exact expected values (tolerance 1e-4 for refinement precision)
        assert abs(refined_result.price - target_price) < 1e-4, (
            f"Refined price should match target: {refined_result.price} vs {target_price}"
        )
        # Vol should match the true vol (0.2) that was used as initial guess
        assert abs(refined_result.vol - true_vol) < 1e-4, (
            f"Refined vol should match true vol: {refined_result.vol} vs {true_vol}"
        )

    def test_refine_vol_and_greeks_short_expiry(self):
        # Test refine_vol_and_greeks with short expiry (challenging case)
        s = 100.0
        r = 0.05
        b = 0.05
        true_vol = 0.3
        is_call = True
        k = 100.0
        t = 0.01  # Very short expiry
        multiplier = 1.0

        true_result = black_scholes_greeks(s, r, b, true_vol, is_call, k, t, multiplier)
        target_price = true_result.price

        # Use a different initial guess
        initial_vol = true_vol * 1.2
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            initial_vol,
            multiplier,
        )

        # Exact expected values (tolerance 5e-2 for short expiry convergence)
        assert abs(refined_result.price - target_price) < 5e-2, (
            f"Refined price should match target: {refined_result.price} vs {target_price}"
        )
        # Vol should converge to the true vol (0.3) that produced the target price
        assert abs(refined_result.vol - true_vol) < 1e-2, (
            f"Refined vol should converge to true vol: {refined_result.vol} vs {true_vol}"
        )

    def test_refine_vol_and_greeks_deep_itm(self):
        # Test refine_vol_and_greeks with deep ITM option
        s = 150.0
        r = 0.05
        b = 0.05
        true_vol = 0.2
        is_call = True
        k = 100.0
        t = 1.0
        multiplier = 1.0

        true_result = black_scholes_greeks(s, r, b, true_vol, is_call, k, t, multiplier)
        target_price = true_result.price

        initial_vol = true_vol * 0.9
        refined_result = refine_vol_and_greeks(
            s,
            r,
            b,
            is_call,
            k,
            t,
            target_price,
            initial_vol,
            multiplier,
        )

        # Exact expected values (tolerance 2e-2 for deep ITM convergence)
        assert abs(refined_result.price - target_price) < 2e-2, (
            f"Refined price should match target: {refined_result.price} vs {target_price}"
        )
        # Vol should converge to the true vol (0.2) that produced the target price
        assert abs(refined_result.vol - true_vol) < 1e-2, (
            f"Refined vol should converge to true vol: {refined_result.vol} vs {true_vol}"
        )
