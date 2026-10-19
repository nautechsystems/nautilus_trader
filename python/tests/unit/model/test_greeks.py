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
"""
Test contract-notional Greeks conversion and Python composition.
"""

import sys

import pytest

from nautilus_trader.model import GreeksData
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import PortfolioGreeks
from nautilus_trader.model import YieldCurve


@pytest.fixture
def greeks() -> GreeksData:
    """
    Return per-unit delta with a non-unit contract multiplier.
    """
    return GreeksData.from_delta(InstrumentId.from_str("ES.GLBX"), 1.25, 8.0, 11)


def test_greeks_data_to_portfolio_greeks(greeks: GreeksData) -> None:
    """
    Apply the contract multiplier without changing the per-unit source.
    """
    result = greeks.to_portfolio_greeks()

    assert isinstance(result, PortfolioGreeks)
    assert (
        result.pnl,
        result.price,
        result.delta,
        result.gamma,
        result.vega,
        result.theta,
        result.rho,
    ) == (
        0.0,
        0.0,
        10.0,
        0.0,
        0.0,
        0.0,
        0.0,
    )
    assert (result.ts_init, result.ts_event) == (11, 11)
    assert (greeks.delta, greeks.multiplier, greeks.quantity) == (1.25, 8.0, 1.0)


@pytest.mark.parametrize(("quantity", "delta"), [(3, 30.0), (-2.0, -20.0), (0, 0.0)])
def test_scalar_left_greeks_data(greeks: GreeksData, quantity: float, delta: float) -> None:
    """
    Scale converted contract Greeks by signed quantity.
    """
    result = quantity * greeks

    assert isinstance(result, PortfolioGreeks)
    assert (
        result.pnl,
        result.price,
        result.delta,
        result.gamma,
        result.vega,
        result.theta,
        result.rho,
    ) == (
        0.0,
        0.0,
        delta,
        0.0,
        0.0,
        0.0,
        0.0,
    )
    assert (result.ts_init, result.ts_event) == (11, 11)
    assert greeks.delta == 1.25


@pytest.mark.parametrize(("quantity", "delta"), [(3, 30.0), (-2.0, -20.0), (0, 0.0)])
def test_scalar_left_portfolio_greeks(greeks: GreeksData, quantity: float, delta: float) -> None:
    """
    Scale portfolio values without reapplying the contract multiplier.
    """
    portfolio = greeks.to_portfolio_greeks()
    result = quantity * portfolio

    assert isinstance(result, PortfolioGreeks)
    assert (
        result.pnl,
        result.price,
        result.delta,
        result.gamma,
        result.vega,
        result.theta,
        result.rho,
    ) == (
        0.0,
        0.0,
        delta,
        0.0,
        0.0,
        0.0,
        0.0,
    )
    assert (result.ts_init, result.ts_event) == (11, 11)
    assert portfolio.delta == 10.0


def test_portfolio_greeks_addition(greeks: GreeksData) -> None:
    """
    Combine portfolio values while retaining the left timestamps.
    """
    left = greeks.to_portfolio_greeks()
    right = GreeksData.from_delta(
        InstrumentId.from_str("NQ.GLBX"),
        0.5,
        4.0,
        13,
    ).to_portfolio_greeks()
    result = left + right

    assert isinstance(result, PortfolioGreeks)
    assert (
        result.pnl,
        result.price,
        result.delta,
        result.gamma,
        result.vega,
        result.theta,
        result.rho,
    ) == (
        0.0,
        0.0,
        12.0,
        0.0,
        0.0,
        0.0,
        0.0,
    )
    assert (result.ts_init, result.ts_event) == (11, 11)
    assert (left.delta, right.delta, right.ts_init, right.ts_event) == (10.0, 2.0, 13, 13)


@pytest.mark.parametrize("operand", ["invalid", object(), None])
def test_greeks_incompatible_operands(greeks: GreeksData, operand: object) -> None:
    """
    Reject operands that cannot participate in Greeks arithmetic.
    """
    portfolio = greeks.to_portfolio_greeks()

    with pytest.raises(TypeError):
        operand * greeks
    with pytest.raises(TypeError):
        operand * portfolio
    with pytest.raises(TypeError):
        portfolio + operand


@pytest.mark.parametrize("portfolio", [False, True])
def test_greeks_right_scalar_multiplication_unsupported(
    greeks: GreeksData,
    portfolio: bool,
) -> None:
    """
    Keep scalar multiplication limited to the scalar-left interface.
    """
    data = greeks.to_portfolio_greeks() if portfolio else greeks

    with pytest.raises(TypeError):
        data * 2.0


@pytest.mark.parametrize(
    ("tenors", "interest_rates", "message"),
    [
        ([0.5, 1.0, 2.0], [0.04, 0.05], "'tenors' usize of 3 was not equal"),
        ([0.5, 1.0], [0.04, 0.05], "one rate or at least three points"),
        ([], [], "one rate or at least three points"),
        ([0.25, 1.0, 5.0], [0.04, float("nan"), 0.05], "must be finite"),
        ([0.05, 0.05, 1.0], [0.04, 0.05, 0.06], "strictly increasing"),
        ([1.0, 1.0 + sys.float_info.epsilon, 2.0], [0.04, 0.05, 0.06], "wide enough"),
    ],
)
def test_yield_curve_rejects_shapes_that_cannot_interpolate(
    tenors: list[float],
    interest_rates: list[float],
    message: str,
) -> None:
    """
    Reject curves the calculator cannot evaluate.
    """
    with pytest.raises(ValueError, match=message):
        YieldCurve("USD", tenors, interest_rates, 1, 2)
