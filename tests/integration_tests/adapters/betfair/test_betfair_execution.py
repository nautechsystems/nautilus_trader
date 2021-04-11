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
from nautilus_trader.model.events import OrderCancelled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
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
    command = BetfairTestStubs.submit_order_command()
    execution_client.submit_order(command)
    await asyncio.sleep(0.01)
    assert isinstance(exec_engine.events[0], OrderSubmitted)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": command.id.value,
        "customer_strategy_ref": command.strategy_id.value,
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
                "customerOrderRef": command.order.client_order_id.value.replace(
                    "-", ""
                ),
            }
        ],
    }
    mock_place_orders.assert_called_with(**expected)


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
    assert execution_client


@pytest.mark.asyncio
async def test_update_order(mocker, execution_client, exec_engine):
    # Add sample order to the cache
    order = BetfairTestStubs.make_order(exec_engine)
    order.apply(BetfairTestStubs.event_order_submitted(order=order))
    order.apply(
        BetfairTestStubs.event_order_accepted(
            order=order, venue_order_id=VenueOrderId("229435133092")
        )
    )
    exec_engine.cache.add_order(order, PositionId("1"))

    mock_replace_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairTestStubs.place_orders_success(),
    )

    # Actual test
    update = BetfairTestStubs.update_order_command(
        instrument_id=order.instrument_id, client_order_id=order.client_order_id
    )
    execution_client.update_order(update)
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": update.id.value,
        "instructions": [{"betId": "229435133092", "newPrice": 1.35}],
        "market_id": "1.179082386",
    }
    mock_replace_orders.assert_called_with(**expected)


@pytest.mark.asyncio
async def test_post_order_update_success(execution_client, exec_engine):
    # Add fake order to cache
    order = BetfairTestStubs.make_order(exec_engine)
    order.apply(BetfairTestStubs.event_order_submitted(order=order))
    order.apply(
        BetfairTestStubs.event_order_accepted(
            order=order, venue_order_id=VenueOrderId("229435133092")
        )
    )
    exec_engine.cache.add_order(order, PositionId("1"))
    client_order_id = exec_engine.cache.orders()[0].client_order_id

    f = asyncio.Future()
    f.set_result(BetfairTestStubs.replace_orders_resp_success())
    execution_client._post_update_order(f, client_order_id)
    await asyncio.sleep(0)
    event = exec_engine.events[0]
    assert isinstance(event, OrderUpdated)
    assert event.price == Price("0.47619")


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
    command = BetfairTestStubs.cancel_order_command()
    execution_client.cancel_order(command)
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": command.id.value,
        "instructions": [{"betId": "229597791245"}],
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


def _prefill_venue_order_id_to_client_order_id(raw):
    order_ids = [
        update["id"]
        for market in raw["oc"]
        for order in market["orc"]
        for update in order.get("uo", [])
    ]
    return {oid: ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}


@pytest.mark.asyncio
async def test_order_stream_full_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_FULL_IMAGE()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
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
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 3


@pytest.mark.asyncio
async def test_order_stream_sub_image(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_SUB_IMAGE()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
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
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.01)
    assert len(exec_engine.events) == 1


@pytest.mark.asyncio
async def test_order_stream_cancel_after_update_doesnt_emit_event(
    mocker, execution_client, exec_engine
):
    raw = BetfairTestStubs.streaming_ocm_order_update()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    s = set()
    s.add(("O-20210409-070830-001-001-1", "229506163591"))
    mocker.patch.object(
        execution_client,
        "pending_update_order_client_ids",
        s,
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.01)
    assert len(exec_engine.events) == 0


@pytest.mark.asyncio
async def test_order_stream_filled(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_FILLED()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.01)
    assert len(exec_engine.events) == 1
    event = exec_engine.events[0]
    assert isinstance(event, OrderFilled)
    assert event.last_px == Price(0.90909, precision=5)


@pytest.mark.asyncio
async def test_order_stream_mixed(mocker, execution_client, exec_engine):
    raw = BetfairTestStubs.streaming_ocm_MIXED()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.1)
    assert len(exec_engine.events) == 3
    assert isinstance(exec_engine.events[0], OrderFilled)
    assert isinstance(exec_engine.events[1], OrderFilled)
    assert isinstance(exec_engine.events[2], OrderCancelled)


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
        "venue_order_id_to_client_order_id",
        {"226125004209": ClientOrderId("1")},
    )

    result = await generate_trades_list(
        self=execution_client, venue_order_id="226125004209", symbol=None, since=None
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
