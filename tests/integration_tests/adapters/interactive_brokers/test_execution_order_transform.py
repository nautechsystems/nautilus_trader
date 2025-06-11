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

from unittest.mock import Mock

import pytest

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.test_kit.stubs.data import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


_AAPL = TestInstrumentProvider.equity("AAPL", "NASDAQ")
_EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("IDEALPRO"))


@pytest.mark.parametrize(
    "expected_order_type, expected_tif, nautilus_order",
    [
        # fmt: off
        ("MKT", "GTC", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.GTC)),
        ("MKT", "DAY", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.DAY)),
        ("MKT", "IOC", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.IOC)),
        ("MKT", "FOK", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.FOK)),
        ("MKT", "OPG", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_OPEN)),
        ("MOC", "DAY", TestExecStubs.market_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_CLOSE)),
        # fmt: on
    ],
)
@pytest.mark.asyncio
async def test_transform_order_to_ib_order_market(
    expected_order_type,
    expected_tif,
    nautilus_order,
    exec_client,
):
    # Arrange
    await exec_client._instrument_provider.load_async(nautilus_order.instrument_id)

    # Act
    ib_order = exec_client._transform_order_to_ib_order(nautilus_order)

    # Assert
    assert (
        ib_order.orderType == expected_order_type
    ), f"{expected_order_type=}, but got {ib_order.orderType=}"
    assert ib_order.tif == expected_tif, f"{expected_tif=}, but got {ib_order.tif=}"


@pytest.mark.parametrize(
    "expected_order_type, expected_tif, nautilus_order",
    [
        # fmt: off
        ("LMT", "GTC", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.GTC)),
        ("LMT", "DAY", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.DAY)),
        ("LMT", "IOC", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.IOC)),
        ("LMT", "FOK", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.FOK)),
        ("LMT", "OPG", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_OPEN)),
        ("LOC", "DAY", TestExecStubs.limit_order(instrument=_EURUSD, time_in_force=TimeInForce.AT_THE_CLOSE)),
        # fmt: on
    ],
)
@pytest.mark.asyncio
async def test_transform_order_to_ib_order_limit(
    expected_order_type,
    expected_tif,
    nautilus_order,
    exec_client,
):
    # Arrange
    await exec_client._instrument_provider.load_async(nautilus_order.instrument_id)

    # Act
    ib_order = exec_client._transform_order_to_ib_order(nautilus_order)

    # Assert
    assert (
        ib_order.orderType == expected_order_type
    ), f"{expected_order_type=}, but got {ib_order.orderType=}"
    assert ib_order.tif == expected_tif, f"{expected_tif=}, but got {ib_order.tif=}"


# Tests for bracket order zero quantity fix
@pytest.mark.asyncio
async def test_transform_order_to_ib_order_zero_quantity_raises_error(exec_client):
    """
    Test that transforming an order with zero quantity raises ValueError.
    """
    # Arrange
    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("IDEALPRO"))
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create a mock order that returns zero quantity
    # This simulates what happens in bracket orders when quantities are updated
    mock_order = Mock()
    mock_order.instrument_id = instrument.id
    mock_order.quantity = Quantity.from_str("0.0")  # Zero quantity!
    mock_order.is_post_only = False
    mock_order.time_in_force = TimeInForce.GTC
    mock_order.price = None
    mock_order.order_type = OrderType.STOP_MARKET
    mock_order.side = OrderSide.SELL
    mock_order.trigger_price = Price.from_str("1.0500")

    # Act & Assert
    with pytest.raises(ValueError, match="Cannot transform order with zero or negative quantity"):
        exec_client._transform_order_to_ib_order(mock_order)


@pytest.mark.asyncio
async def test_transform_order_to_ib_order_negative_quantity_raises_error(exec_client):
    """
    Test that transforming an order with negative quantity raises ValueError.
    """
    # Arrange
    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("IDEALPRO"))
    await exec_client._instrument_provider.load_async(instrument.id)

    # Create a mock order that returns negative quantity
    # This simulates what happens in bracket orders when quantities are updated incorrectly
    mock_order = Mock()
    mock_order.instrument_id = instrument.id
    # Create a mock quantity that returns negative value
    mock_quantity = Mock()
    mock_quantity.as_double.return_value = -1.0
    mock_order.quantity = mock_quantity
    mock_order.is_post_only = False
    mock_order.time_in_force = TimeInForce.GTC
    mock_order.price = None
    mock_order.order_type = OrderType.STOP_MARKET
    mock_order.side = OrderSide.SELL
    mock_order.trigger_price = Price.from_str("1.0500")

    # Act & Assert
    with pytest.raises(ValueError, match="Cannot transform order with zero or negative quantity"):
        exec_client._transform_order_to_ib_order(mock_order)


# Note: Additional tests for modify_order and parse_ib_order_to_order_status_report
# zero quantity handling are covered by the implementation but are difficult to test
# in isolation due to the protected nature of the execution client's internal state.
# The core functionality is tested through the transform_order_to_ib_order tests above.


@pytest.mark.asyncio
async def test_transform_order_to_ib_order_positive_quantity_works_normally(exec_client):
    """
    Test that transforming an order with positive quantity works normally.
    """
    # Arrange
    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("IDEALPRO"))
    await exec_client._instrument_provider.load_async(instrument.id)

    stop_order = StopMarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("STRATEGY-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("STOP-001"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("1.0"),  # Positive quantity
        trigger_price=Price.from_str("1.0500"),
        trigger_type=TriggerType.DEFAULT,
        init_id=UUID4(),
        ts_init=0,
    )

    # Act
    ib_order = exec_client._transform_order_to_ib_order(stop_order)

    # Assert
    assert ib_order is not None
    assert ib_order.totalQuantity == 1.0
    assert ib_order.orderType == "STP"
    assert ib_order.action == "SELL"
