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
import os

import betfairlightweight
import orjson
import pytest

from nautilus_trader.adapters.betfair.parsing import generate_trades_list
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.identifiers import ClientOrderId
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.asyncio
@pytest.mark.skip  # Only runs locally, comment to run
async def test_client_connect(live_logger):
    betfair_client = betfairlightweight.APIClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PW"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        certs=os.environ["BETFAIR_CERT_DIR"],
    )
    # TODO - mock login won't let you login - need to comment out in conftest.py to run
    betfair_client.login()
    socket = BetfairMarketStreamClient(
        client=betfair_client, logger=live_logger, message_handler=print
    )
    await socket.connect()
    await socket.send_subscription_message(market_ids=["1.180634014"])
    await asyncio.sleep(15)


@pytest.mark.asyncio
async def test_submit_order(mocker, execution_client, exec_engine):
    mock_place_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.place_orders",
        return_value=BetfairTestStubs.place_orders_success(),
    )
    execution_client.submit_order(BetfairTestStubs.submit_order_command())
    await asyncio.sleep(0.1)
    assert isinstance(exec_engine.events[0], OrderSubmitted)
    assert isinstance(exec_engine.events[1], OrderAccepted)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": "1",
        "customer_strategy_ref": "1",
        "instructions": [
            {
                "orderType": "LIMIT",
                "selectionId": "50214",
                "side": "BACK",
                "handicap": "0",
                "limitOrder": {
                    "price": 3.05,
                    "persistenceType": "PERSIST",
                    "size": 10.0,
                    "minFillSize": 0,
                },
                "customerOrderRef": "1",
            }
        ],
    }
    mock_place_orders.assert_called_with(**expected)


@pytest.mark.asyncio
@pytest.mark.skip  # Stuggled to test this, couldn't get an order into the cache as required by update_order
async def test_update_order(mocker, execution_client, exec_engine):
    mock_replace_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairTestStubs.place_orders_success(),
    )

    # Order must exist in cache - add one
    command = BetfairTestStubs.submit_order_command()
    exec_engine.execute(command=command)
    await asyncio.sleep(0)
    assert exec_engine.cache.orders

    # Actual test
    update = BetfairTestStubs.update_order_command(
        instrument_id=command.order.instrument_id,
        cl_ord_id=command.order.cl_ord_id,
    )
    execution_client.update_order(update)
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": "001",
        "instructions": [{"betId": "1", "newPrice": 1.35}],
        "market_id": "1.179082386",
    }
    mock_replace_orders.assert_called_with(**expected)


@pytest.mark.asyncio
async def test_update_order_fail(mocker, execution_client, exec_engine):
    execution_client.update_order(BetfairTestStubs.update_order_command())
    await asyncio.sleep(0.1)
    mock_replace_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairTestStubs.place_orders_success(),
    )
    mock_replace_orders.assert_not_called()


@pytest.mark.asyncio
async def test_cancel_order(mocker, execution_client, exec_engine):
    mock_cancel_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.cancel_orders",
        return_value=BetfairTestStubs.cancel_orders_success(),
    )
    execution_client.cancel_order(BetfairTestStubs.cancel_order_command())
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": "038990c6-19d2-b5c8-37a6-fe91f9b7b9ed",
        "instructions": [{"betId": "1"}],
        "market_id": "1.179082386",
    }

    mock_cancel_orders.assert_called_with(**expected)


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
        for update in order.get("uo", [])
    ]
    return {oid: ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}


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


@pytest.mark.asyncio
async def test_order_stream_sub_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_SUB_IMAGE()
    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        _prefill_order_id_to_cl_ord_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert (
        len(exec_engine.events) == 0
    )  # We don't do anything with matched bets at this stage


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


@pytest.mark.asyncio
async def test_post_order_submit_success(execution_client, exec_engine):
    f = asyncio.Future()
    f.set_result(BetfairTestStubs.place_orders_success())
    execution_client._post_submit_order(f, ClientOrderId("O-20210327-091154-001-001-2"))
    await asyncio.sleep(0)
    assert isinstance(exec_engine.events[0], OrderAccepted)


@pytest.mark.asyncio
async def test_post_order_submit_error(execution_client, exec_engine):
    f = asyncio.Future()
    f.set_result(BetfairTestStubs.place_orders_error())
    execution_client._post_submit_order(f, ClientOrderId("O-20210327-091154-001-001-2"))
    await asyncio.sleep(0)
    assert isinstance(exec_engine.events[0], OrderRejected)


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


@pytest.mark.asyncio
async def test_generate_trades_list(mocker, execution_client):
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_cleared_orders",
        return_value=BetfairTestStubs.list_cleared_orders(order_id="226125004209"),
    )
    mocker.patch.object(
        execution_client,
        "order_id_to_cl_ord_id",
        {"226125004209": ClientOrderId("1")},
    )

    result = await generate_trades_list(
        self=execution_client, order_id="226125004209", symbol=None, since=None
    )
    assert result


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
