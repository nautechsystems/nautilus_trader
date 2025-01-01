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
"""
Unit tests for the parsing methods used by the dYdX adapter.
"""

import pytest

from nautilus_trader.adapters.dydx.common.enums import DYDXCandlesResolution
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXLiquidity
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderSide
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXOrderType
from nautilus_trader.adapters.dydx.common.enums import DYDXPositionSide
from nautilus_trader.adapters.dydx.common.enums import DYDXTimeInForce
from nautilus_trader.adapters.dydx.common.parsing import get_interval_from_bar_type
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce


@pytest.fixture
def enum_parser() -> DYDXEnumParser:
    """
    Create an enum parser.
    """
    return DYDXEnumParser()


@pytest.mark.parametrize(
    ("dydx_order_type", "expected_result"),
    [
        (DYDXOrderType.LIMIT, OrderType.LIMIT),
        (DYDXOrderType.MARKET, OrderType.MARKET),
        (DYDXOrderType.STOP_LIMIT, OrderType.STOP_LIMIT),
        (DYDXOrderType.STOP_MARKET, OrderType.STOP_MARKET),
    ],
)
def test_parse_order_type(
    dydx_order_type: DYDXOrderType,
    expected_result: OrderType,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx order types to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_order_type(dydx_order_type)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("order_type", "expected_result"),
    [
        (OrderType.LIMIT, DYDXOrderType.LIMIT),
        (OrderType.MARKET, DYDXOrderType.MARKET),
        (OrderType.STOP_LIMIT, DYDXOrderType.STOP_LIMIT),
        (OrderType.STOP_MARKET, DYDXOrderType.STOP_MARKET),
    ],
)
def test_parse_nautilus_order_type(
    order_type: OrderType,
    expected_result: DYDXOrderType,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the Nautilus order types to dYdX order types.
    """
    # Act
    result = enum_parser.parse_nautilus_order_type(order_type)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("dydx_order_side", "expected_result"),
    [
        (DYDXOrderSide.BUY, OrderSide.BUY),
        (DYDXOrderSide.SELL, OrderSide.SELL),
        (None, OrderSide.NO_ORDER_SIDE),
    ],
)
def test_parse_order_side(
    dydx_order_side: DYDXOrderSide | None,
    expected_result: OrderSide,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx order side to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_order_side(dydx_order_side)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("order_side", "expected_result"),
    [
        (OrderSide.BUY, DYDXOrderSide.BUY),
        (OrderSide.SELL, DYDXOrderSide.SELL),
        (OrderSide.NO_ORDER_SIDE, None),
    ],
)
def test_parse_nautilus_order_side(
    order_side: OrderSide,
    expected_result: DYDXOrderSide | None,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the Nautilus order side to dYdX order side.
    """
    # Act
    result = enum_parser.parse_nautilus_order_side(order_side)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("dydx_order_status", "expected_result"),
    [
        (DYDXOrderStatus.OPEN, OrderStatus.ACCEPTED),
        (DYDXOrderStatus.FILLED, OrderStatus.FILLED),
        (DYDXOrderStatus.CANCELED, OrderStatus.CANCELED),
        (DYDXOrderStatus.BEST_EFFORT_CANCELED, OrderStatus.PENDING_CANCEL),
        (DYDXOrderStatus.BEST_EFFORT_OPENED, OrderStatus.ACCEPTED),
        (DYDXOrderStatus.UNTRIGGERED, OrderStatus.ACCEPTED),
    ],
)
def test_parse_order_status(
    dydx_order_status: DYDXOrderStatus,
    expected_result: OrderStatus,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx order status to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_order_status(dydx_order_status)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("dydx_time_in_force", "expected_result"),
    [
        (DYDXTimeInForce.GTT, TimeInForce.GTD),
        (DYDXTimeInForce.FOK, TimeInForce.FOK),
        (DYDXTimeInForce.IOC, TimeInForce.IOC),
    ],
)
def test_parse_order_time_in_force(
    dydx_time_in_force: DYDXTimeInForce,
    expected_result: TimeInForce,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx order time in force to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_time_in_force(dydx_time_in_force)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("dydx_liquidity", "expected_result"),
    [
        (DYDXLiquidity.MAKER, LiquiditySide.MAKER),
        (DYDXLiquidity.TAKER, LiquiditySide.TAKER),
    ],
)
def test_parse_dydx_liquidity_side(
    dydx_liquidity: DYDXLiquidity,
    expected_result: LiquiditySide,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx order time in force to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_liquidity_side(dydx_liquidity)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("dydx_position_side", "expected_result"),
    [
        (DYDXPositionSide.LONG, PositionSide.LONG),
        (DYDXPositionSide.SHORT, PositionSide.SHORT),
    ],
)
def test_parse_dydx_position_side(
    dydx_position_side: DYDXPositionSide,
    expected_result: PositionSide,
    enum_parser: DYDXEnumParser,
) -> None:
    """
    Test converting the dydx position side to nautilus types.
    """
    # Act
    result = enum_parser.parse_dydx_position_side(dydx_position_side)

    # Assert
    assert result == expected_result


@pytest.mark.parametrize(
    ("bar_type", "dydx_kline_interval"),
    [
        ("ETH-USD-PERP.DYDX-1-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.ONE_MINUTE),
        ("ETH-USD-PERP.DYDX-5-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.FIVE_MINUTES),
        ("ETH-USD-PERP.DYDX-15-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.FIFTEEN_MINUTES),
        ("ETH-USD-PERP.DYDX-30-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.THIRTY_MINUTES),
        ("ETH-USD-PERP.DYDX-60-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.ONE_HOUR),
        ("ETH-USD-PERP.DYDX-240-MINUTE-LAST-EXTERNAL", DYDXCandlesResolution.FOUR_HOURS),
        ("ETHUSDT.BYBIT-1-HOUR-LAST-EXTERNAL", DYDXCandlesResolution.ONE_HOUR),
        ("ETHUSDT.BYBIT-4-HOUR-LAST-EXTERNAL", DYDXCandlesResolution.FOUR_HOURS),
        ("ETHUSDT.BYBIT-24-HOUR-LAST-EXTERNAL", DYDXCandlesResolution.ONE_DAY),
        ("ETH-USDT-PERP.DYDX-1-DAY-LAST-EXTERNAL", DYDXCandlesResolution.ONE_DAY),
    ],
)
def test_parse_dydx_kline_correct(
    bar_type: str,
    dydx_kline_interval: DYDXCandlesResolution,
) -> None:
    """
    Test parsing the nautilus bar type to a dydx kline interval.
    """
    bar_type = BarType.from_str(bar_type)
    result: DYDXCandlesResolution = get_interval_from_bar_type(bar_type)
    assert result == dydx_kline_interval


@pytest.mark.parametrize(
    "bar_type",
    [
        "ETH-USD-PERP.DYDX-2-MINUTE-LAST-EXTERNAL",
        "ETH-USD-PERP.DYDX-3-HOUR-LAST-EXTERNAL",
        "ETH-USD-PERP.DYDX-3-DAY-LAST-EXTERNAL",
        "ETH-USD-PERP.DYDX-2-WEEK-LAST-EXTERNAL",
        "ETH-USD-PERP.DYDX-4-MONTH-LAST-EXTERNAL",
    ],
)
def test_parse_dydx_kline_incorrect(bar_type: str) -> None:
    """
    Test bar types which are not supported by dYdX.
    """
    with pytest.raises(KeyError):
        get_interval_from_bar_type(BarType.from_str(bar_type))
