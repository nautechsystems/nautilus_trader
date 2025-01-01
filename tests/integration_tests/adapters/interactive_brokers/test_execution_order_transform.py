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

from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import Venue
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
