# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime

import pytest

from adapters.betfair.common import order_submit_to_betfair
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"


@pytest.mark.fixture(autouse=True)
def setup():
    # TODO Mock betfairlightweight request/response
    pass


def test_order_submit_to_betfair(
    trader_id,
    account_id,
    strategy_id,
    position_id,
    instrument_id,
    uuid,
    betting_instrument,
):
    command = SubmitOrder(
        instrument_id=instrument_id,
        trader_id=trader_id,
        account_id=account_id,
        strategy_id=strategy_id,
        position_id=position_id,
        order=LimitOrder(
            cl_ord_id=ClientOrderId("1"),
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(10),
            price=Price(100),
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            init_id=uuid,
            timestamp=datetime.datetime.now(),
        ),
        command_id=uuid,
        command_timestamp=datetime.datetime.now(),
    )
    result = order_submit_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": "1",
        "customer_strategy_ref": "Test-1",
        "instructions": [
            {
                "orderType": "LIMIT",
                "selectionId": 50214,
                "side": "Back",
                "handicap": "0.0",
                "limitOrder": {
                    "price": 95.0,
                    "persistenceType": "PERSIST",
                    "size": 10.0,
                    "minFillSize": 0,
                },
                "customerOrderRef": "1",
            }
        ],
    }
    assert result == expected


def test_account_statement():
    pass


#
# def test_connect(self):
#     # Arrange
#     # Act
#     self.exec_engine.start()  # Also connects clients
#     await asyncio.sleep(0.3)  # Allow engine message queue to start
#
#     # Assert
#     self.assertTrue(self.client.is_connected)
#
#     # Tear down
#     self.exec_engine.stop()
#     await self.exec_engine.get_run_queue_task()
#
#
#
# def test_disconnect(self):
#     # Arrange
#     self.exec_engine.start()
#     await asyncio.sleep(0.3)  # Allow engine message queue to start
#
#     # Act
#     self.client.disconnect()
#     await asyncio.sleep(0.3)
#
#     # Assert
#     self.assertFalse(self.client.is_connected)
#
#     # Tear down
#     self.exec_engine.stop()
#     await self.exec_engine.get_run_queue_task()
#
#
# def test_reset_when_not_connected_successfully_resets(self):
#     # Arrange
#     self.exec_engine.start()
#     await asyncio.sleep(0.3)  # Allow engine message queue to start
#
#     self.exec_engine.stop()
#     await asyncio.sleep(0.3)  # Allow engine message queue to stop
#
#     # Act
#     self.client.reset()
#
#     # Assert
#     self.assertFalse(self.client.is_connected)
#
#
# def test_reset_when_connected_does_not_reset(self):
#     # Arrange
#     self.exec_engine.start()
#     await asyncio.sleep(0.3)  # Allow engine message queue to start
#
#     # Act
#     self.client.reset()
#
#     # Assert
#     self.assertTrue(self.client.is_connected)
#
#     # Tear Down
#     self.exec_engine.stop()
#     await self.exec_engine.get_run_queue_task()
#
#
# def test_dispose_when_not_connected_does_not_dispose(self):
#     # Arrange
#     self.exec_engine.start()
#     await asyncio.sleep(0.3)  # Allow engine message queue to start
#
#     # Act
#     self.client.dispose()
#
#     # Assert
#     self.assertTrue(self.client.is_connected)
#
#     # Tear Down
#     self.exec_engine.stop()
#     await self.exec_engine.get_run_queue_task()
