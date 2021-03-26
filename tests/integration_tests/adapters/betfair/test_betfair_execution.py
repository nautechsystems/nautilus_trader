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
import asyncio
import datetime
import os

import betfairlightweight
import orjson
import pytest

from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import order_amend_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.asyncio
@pytest.mark.skip  # Only runs locally, comment to run
async def test_connect():
    betfair_client = betfairlightweight.APIClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PW"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        certs=os.environ["BETFAIR_CERT_DIR"],
    )
    # TODO - mock login won't let you login - need to comment out in conftest.py to run
    betfair_client.login()
    socket = BetfairMarketStreamClient(client=betfair_client, message_handler=print)
    await socket.connect()
    await socket.send_subscription_message(market_ids=["1.180634014"])
    await asyncio.sleep(15)


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
            price=Price(0.33, 5),
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
        "async": True,
        "customer_ref": "1",
        "customer_strategy_ref": "BETFAIR-001-Test-1",
        "instructions": [
            {
                "customerOrderRef": "1",
                "handicap": "0.0",
                "limitOrder": {
                    "minFillSize": 0,
                    "persistenceType": "PERSIST",
                    "price": 3.05,
                    "size": 10.0,
                },
                "orderType": "LIMIT",
                "selectionId": 50214,
                "side": "Back",
            }
        ],
        "market_id": "1.179082386",
    }
    assert result == expected


def test_order_amend_to_betfair(
    trader_id,
    account_id,
    strategy_id,
    position_id,
    instrument_id,
    uuid,
    betting_instrument,
):
    command = AmendOrder(
        instrument_id=instrument_id,
        trader_id=trader_id,
        account_id=account_id,
        cl_ord_id=ClientOrderId("1"),
        quantity=Quantity(50),
        price=Price(20),
        command_id=uuid,
        command_timestamp=datetime.datetime.now(),
    )
    result = order_amend_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": result["customer_ref"],
        "async": True,
        "instructions": [{"betId": "1", "newPrice": 20.0}],
    }

    assert result == expected


def test_order_cancel_to_betfair(
    trader_id,
    account_id,
    instrument_id,
    uuid,
    betting_instrument,
):
    cl_orr_id = ClientOrderId("1")
    order_id = OrderId("1")
    command = CancelOrder(
        instrument_id,
        trader_id,
        account_id,
        cl_orr_id,
        order_id,
        uuid,
        datetime.datetime.now(),
    )
    result = order_cancel_to_betfair(command=command, instrument=betting_instrument)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": result["customer_ref"],
        "instructions": [
            {
                "betId": "1",
            }
        ],
    }
    assert result == expected


def test_account_statement(betfair_client, uuid):
    detail = betfair_client.account.get_account_details()
    funds = betfair_client.account.get_account_funds()
    result = betfair_account_to_account_state(
        account_detail=detail,
        account_funds=funds,
        event_id=uuid,
    )
    expected = AccountState(
        AccountId(issuer="betfair", identifier="Testy-McTest"),
        [Money(1000.0, Currency.from_str("AUD"))],
        [Money(1000.0, Currency.from_str("AUD"))],
        [Money(-0.00, Currency.from_str("AUD"))],
        {"funds": funds, "detail": detail},
        uuid,
        result.timestamp,
    )
    assert result == expected


@pytest.mark.asyncio
async def test_connection_account_state(execution_client, exec_engine):
    await execution_client.connection_account_state()
    assert isinstance(exec_engine.events[0], AccountState)


def test_get_account_currency(execution_client):
    currency = execution_client.get_account_currency()
    assert currency == "AUD"


def _prefill_order_id_to_cl_ord_id(raw):
    order_ids = [
        update["id"]
        for market in raw["oc"]
        for order in market["orc"]
        for update in order["uo"]
    ]
    return {oid: ClientOrderId(str(i)) for i, oid in enumerate(order_ids)}


# TODO - could add better assertions here to ensure all fields are flowing through correctly on at least 1 order?
@pytest.mark.asyncio
async def test_order_stream_full_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_FULL_IMAGE()
    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        _prefill_order_id_to_cl_ord_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 6


@pytest.mark.asyncio
async def test_order_stream_empty_image(execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_EMPTY_IMAGE()
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 0


@pytest.mark.asyncio
async def test_order_stream_new_full_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_NEW_FULL_IMAGE()
    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        _prefill_order_id_to_cl_ord_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 3


# TODO - This test is breaking because we receive a fill update without any information about the order
@pytest.mark.asyncio
@pytest.mark.skip
async def test_order_stream_sub_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_SUB_IMAGE()
    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        _prefill_order_id_to_cl_ord_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 1


@pytest.mark.asyncio
async def test_order_stream_update(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_UPDATE()

    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        _prefill_order_id_to_cl_ord_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 2


# TODO
@pytest.mark.asyncio
@pytest.mark.skip
async def test_generate_order_status_report(mocker, execution_client):
    # Betfair client login
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_current_orders",
        return_value=BetfairTestStubs.current_orders(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_current_orders",
        return_value=BetfairTestStubs.current_orders(),
    )
    result = await execution_client.generate_order_status_report()
    assert result
    raise NotImplementedError()


# TODO
@pytest.mark.asyncio
@pytest.mark.skip
async def test_generate_trades_list(mocker, execution_client):
    mocker.patch("betfairlightweight.endpoints.betting.Betting.list_cleared_orders")
    result = await execution_client.generate_trades_list()
    assert result
    raise NotImplementedError()


# TODO - test that we can concurrently insert orders into an IN-PLAY game
@pytest.mark.skip()
def test_live_order_insert_concurrent():
    pass


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
