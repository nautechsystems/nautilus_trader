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

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.test_kit.rust.orders_pyo3 import TestOrderProviderPyo3


AUDUSD_SIM = InstrumentId.from_str("AUD/USD.SIM")
trader_id = TraderId("TESTER-000")
strategy_id = StrategyId("S-001")
account_id = AccountId("SIM-000")


################################################################################
# MarketOrder
################################################################################


@pytest.mark.parametrize(
    ("side", "expected"),
    [
        [OrderSide.BUY, OrderSide.SELL],
        [OrderSide.SELL, OrderSide.BUY],
    ],
)
def test_opposite_side_returns_expected_sides(side, expected):
    # Arrange, Act
    result = nautilus_pyo3.MarketOrder.opposite_side(side)

    # Assert
    assert result == expected


@pytest.mark.parametrize(
    ("side", "expected"),
    [
        [PositionSide.LONG, OrderSide.SELL],
        [PositionSide.SHORT, OrderSide.BUY],
    ],
)
def test_closing_side_returns_expected_sides(
    side: PositionSide,
    expected: OrderSide,
) -> None:
    # Arrange, Act
    result = nautilus_pyo3.MarketOrder.closing_side(side)

    # Assert
    assert result == expected


@pytest.mark.parametrize(
    ("order_side", "position_side", "position_qty", "expected"),
    [
        [OrderSide.BUY, PositionSide.FLAT, Quantity.from_int(0), False],
        [OrderSide.BUY, PositionSide.SHORT, Quantity.from_str("0.5"), False],
        [OrderSide.BUY, PositionSide.SHORT, Quantity.from_int(1), True],
        [OrderSide.BUY, PositionSide.SHORT, Quantity.from_int(2), True],
        [OrderSide.BUY, PositionSide.LONG, Quantity.from_int(2), False],
        [OrderSide.SELL, PositionSide.SHORT, Quantity.from_int(2), False],
        [OrderSide.SELL, PositionSide.LONG, Quantity.from_int(2), True],
        [OrderSide.SELL, PositionSide.LONG, Quantity.from_int(1), True],
        [OrderSide.SELL, PositionSide.LONG, Quantity.from_str("0.5"), False],
        [OrderSide.SELL, PositionSide.FLAT, Quantity.from_int(0), False],
    ],
)
def test_would_reduce_only_with_various_values_returns_expected(
    order_side,
    position_side,
    position_qty,
    expected,
):
    # Arrange
    order = TestOrderProviderPyo3.market_order(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=AUDUSD_SIM,
        order_side=order_side,
        quantity=Quantity.from_int(1),
    )

    # Act, Assert
    assert order.would_reduce_only(side=position_side, position_qty=position_qty) == expected


def test_market_order_with_quantity_zero_raises_value_error():
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TestOrderProviderPyo3.market_order(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=AUDUSD_SIM,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(0),
        )


def test_market_order_with_invalid_tif_raises_value_error():
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TestOrderProviderPyo3.market_order(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=AUDUSD_SIM,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(0),
            time_in_force=TimeInForce.GTD,
        )


def test_pyo3_cython_conversion():
    market_order_pyo3 = TestOrderProviderPyo3.market_order(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(1),
    )
    market_order_pyo3_dict = market_order_pyo3.to_dict()
    market_order_cython = MarketOrder.from_pyo3(market_order_pyo3)
    market_order_cython_dict = MarketOrder.to_dict(market_order_cython)
    market_order_pyo3_back = nautilus_pyo3.MarketOrder.from_dict(market_order_cython_dict)
    assert market_order_pyo3_dict == market_order_cython_dict
    assert market_order_pyo3 == market_order_pyo3_back
