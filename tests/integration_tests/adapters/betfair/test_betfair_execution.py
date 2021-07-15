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

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing import generate_trades_list
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


# Execution test helpers


def _prefill_venue_order_id_to_client_order_id(raw):
    order_ids = [
        update["id"]
        for market in raw.get("oc", [])
        for order in market["orc"]
        for update in order.get("uo", [])
    ]
    return {oid: ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}


def setup_exec_client_and_cache(mocker, exec_client, exec_engine, logger, raw):
    """
    Parse raw order data and add orders to the execution cache.

    Returns: `venue_order_id_to_client_order_id`
    """
    update = orjson.loads(raw)
    logger.debug(f"raw_data:\n{update}")
    venue_order_ids = _prefill_venue_order_id_to_client_order_id(update)
    venue_order_id_to_client_order_id = {}
    for c_id, v_id in enumerate(venue_order_ids):
        order = BetfairTestStubs.make_accepted_order(
            venue_order_id=v_id, client_order_id=ClientOrderId(str(c_id))
        )
        logger.debug(f"created order: {order}")
        venue_order_id_to_client_order_id[v_id] = order.client_order_id
        logger.debug(f"venue_order_id={v_id}, client_order_id={order.client_order_id}")
        cache_order = exec_engine.cache.order(client_order_id=order.client_order_id)
        if cache_order is None:
            logger.debug("adding to cache")
            exec_engine.cache.add_order(order, position_id=PositionId(v_id))

    mocker.patch.object(
        exec_client, "venue_order_id_to_client_order_id", venue_order_id_to_client_order_id
    )
    return


@pytest.mark.asyncio
@pytest.mark.skip(reason="Local testing only")
async def test_client_connect(live_logger):
    betfair_client = betfairlightweight.APIClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PW"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        certs=os.environ["BETFAIR_CERT_DIR"],
    )
    #  mock login won't let you login - need to comment out in conftest.py to run
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
        return_value=BetfairDataProvider.place_orders_success(),
    )
    command = BetfairTestStubs.submit_order_command()
    execution_client.submit_order(command)
    await asyncio.sleep(0.1)
    assert isinstance(exec_engine.events[0], OrderSubmitted)
    expected = {
        "market_id": "1.179082386",
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": "S-001",
        "instructions": [
            {
                "orderType": "LIMIT",
                "selectionId": "50214",
                "side": "BACK",
                "handicap": "",
                "limitOrder": {
                    "price": 3.05,
                    "persistenceType": "PERSIST",
                    "size": 10.0,
                    "minFillSize": 0,
                },
                "customerOrderRef": "O-20210410-022422-001-001-S",
            }
        ],
    }
    result = mock_place_orders.call_args[1]
    assert result == expected


@pytest.mark.asyncio
async def test_post_order_submit_success(execution_client, exec_engine):
    f = asyncio.Future()
    f.set_result(BetfairDataProvider.place_orders_success())
    execution_client._post_submit_order(
        f,
        BetfairTestStubs.strategy_id(),
        BetfairTestStubs.instrument_id(),
        ClientOrderId("O-20210327-091154-001-001-2"),
    )
    await asyncio.sleep(0)
    assert isinstance(exec_engine.events[0], OrderAccepted)


@pytest.mark.asyncio
async def test_post_order_submit_error(execution_client, exec_engine):
    f = asyncio.Future()
    f.set_result(BetfairDataProvider.place_orders_error())
    execution_client._post_submit_order(
        f,
        BetfairTestStubs.strategy_id(),
        BetfairTestStubs.instrument_id(),
        ClientOrderId("O-20210327-091154-001-001-2"),
    )
    await asyncio.sleep(0)
    assert isinstance(exec_engine.events[0], OrderRejected)
    assert execution_client


@pytest.mark.asyncio
async def test_update_order(mocker, execution_client, exec_engine):
    # Add sample order to the cache
    order = BetfairTestStubs.make_order()
    order.apply(BetfairTestStubs.event_order_submitted(order=order))
    order.apply(
        BetfairTestStubs.event_order_accepted(
            order=order,
            venue_order_id=VenueOrderId("229435133092"),
        )
    )
    exec_engine.cache.add_order(order, PositionId("1"))

    mock_replace_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairDataProvider.place_orders_success(),
    )

    # Actual test
    update = BetfairTestStubs.update_order_command(
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
    )
    execution_client.update_order(update)
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": update.id.value.replace("-", ""),
        "instructions": [{"betId": "229435133092", "newPrice": 1.35}],
        "market_id": "1.179082386",
    }
    mock_replace_orders.assert_called_with(**expected)


@pytest.mark.asyncio
async def test_post_order_update_success(execution_client, exec_engine):
    # Add fake order to cache
    order = BetfairTestStubs.make_order()
    order.apply(BetfairTestStubs.event_order_submitted(order=order))
    order.apply(
        BetfairTestStubs.event_order_accepted(
            order=order,
            venue_order_id=VenueOrderId("229435133092"),
        )
    )
    exec_engine.cache.add_order(order, PositionId("1"))
    client_order_id = exec_engine.cache.orders()[0].client_order_id

    f = asyncio.Future()
    f.set_result(BetfairDataProvider.replace_orders_resp_success())
    execution_client._post_update_order(
        f,
        BetfairTestStubs.strategy_id(),
        BetfairTestStubs.instrument_id(),
        client_order_id,
    )
    await asyncio.sleep(0)
    event = exec_engine.events[0]
    assert isinstance(event, OrderUpdated)
    assert event.price == Price.from_str("0.47619")


@pytest.mark.asyncio
async def test_update_order_fail(mocker, execution_client, exec_engine):
    execution_client.update_order(BetfairTestStubs.update_order_command())
    await asyncio.sleep(0.1)
    mock_replace_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairDataProvider.place_orders_success(),
    )
    mock_replace_orders.assert_not_called()


@pytest.mark.asyncio
async def test_cancel_order(mocker, execution_client, exec_engine):
    mock_cancel_orders = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.cancel_orders",
        return_value=BetfairDataProvider.cancel_orders_success(),
    )
    command = BetfairTestStubs.cancel_order_command()
    execution_client.cancel_order(command)
    await asyncio.sleep(0.1)
    expected = {
        "customer_ref": command.id.value.replace("-", ""),
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
    assert currency == AUD


@pytest.mark.asyncio
async def test_order_stream_full_image(
    mocker, execution_client, exec_engine, order_factory, logger
):
    raw = BetfairDataProvider.streaming_ocm_FULL_IMAGE()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
    )

    # assert
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(1)
    assert len(exec_engine.events) == 12


@pytest.mark.asyncio
async def test_order_stream_empty_image(execution_client, exec_engine):
    raw = BetfairDataProvider.streaming_ocm_EMPTY_IMAGE()
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 0


@pytest.mark.asyncio
async def test_order_stream_new_full_image(
    mocker, execution_client, exec_engine, logger, order_factory
):
    raw = BetfairDataProvider.streaming_ocm_NEW_FULL_IMAGE()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 6


@pytest.mark.asyncio
async def test_order_stream_sub_image(mocker, execution_client, exec_engine):
    raw = BetfairDataProvider.streaming_ocm_SUB_IMAGE()
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        _prefill_venue_order_id_to_client_order_id(orjson.loads(raw)),
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0)
    assert len(exec_engine.events) == 0  # We don't do anything with matched bets at this stage


@pytest.mark.asyncio
async def test_order_stream_update(mocker, execution_client, exec_engine, logger):
    raw = BetfairDataProvider.streaming_ocm_UPDATE()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.1)
    assert len(exec_engine.events) == 1


@pytest.mark.asyncio
async def test_order_stream_cancel_after_update_doesnt_emit_event(
    mocker, execution_client, exec_engine, logger
):
    raw = BetfairDataProvider.streaming_ocm_order_update()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
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
async def test_order_stream_filled(mocker, execution_client, exec_engine, logger):
    raw = BetfairDataProvider.streaming_ocm_FILLED()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.01)
    assert len(exec_engine.events) == 2
    event = exec_engine.events[0]
    assert isinstance(event, OrderFilled)
    assert event.last_px == Price(0.90909, precision=5)


@pytest.mark.asyncio
async def test_order_stream_mixed(mocker, execution_client, exec_engine, logger):
    raw = BetfairDataProvider.streaming_ocm_MIXED()
    setup_exec_client_and_cache(
        mocker=mocker, exec_client=execution_client, exec_engine=exec_engine, logger=logger, raw=raw
    )
    execution_client.handle_order_stream_update(raw=raw)
    await asyncio.sleep(0.5)
    events = exec_engine.events
    assert len(events) == 5
    assert isinstance(events[0], OrderFilled) and events[0].venue_order_id.value == "229430281341"
    assert isinstance(events[1], AccountState)
    assert isinstance(events[2], OrderFilled) and events[2].venue_order_id.value == "229430281339"
    assert isinstance(events[3], AccountState)
    assert isinstance(events[4], OrderCanceled) and events[4].venue_order_id.value == "229430281339"


@pytest.mark.asyncio
@pytest.mark.skip(reason="Not implemented")
async def test_generate_order_status_report(mocker, execution_client):
    # Betfair client login
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_current_orders",
        return_value=BetfairDataProvider.current_orders(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_current_orders",
        return_value=BetfairDataProvider.current_orders(),
    )
    result = await execution_client.generate_order_status_report()
    assert result
    raise NotImplementedError()


@pytest.mark.asyncio
async def test_generate_trades_list(mocker, execution_client):
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_cleared_orders",
        return_value=BetfairDataProvider.list_cleared_orders(order_id="226125004209"),
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


@pytest.mark.asyncio
async def test_duplicate_execution_id(mocker, execution_client, exec_engine, logger):
    mocker.patch.object(
        execution_client,
        "venue_order_id_to_client_order_id",
        {"230486317487": ClientOrderId("1")},
    )

    # Load submitted orders
    kw = {
        "customer_order_ref": "0",
        "bet_id": "230486317487",
    }
    f = asyncio.Future()
    f.set_result(BetfairTestStubs.make_order_place_response())
    execution_client._post_submit_order(
        f,
        BetfairTestStubs.strategy_id(),
        BetfairTestStubs.instrument_id(),
        ClientOrderId(kw["customer_order_ref"]),
    )

    kw = {
        "customer_order_ref": "1",
        "bet_id": "230487922962",
    }
    f = asyncio.Future()
    f.set_result(BetfairTestStubs.make_order_place_response(**kw))
    execution_client._post_submit_order(
        f,
        BetfairTestStubs.strategy_id(),
        BetfairTestStubs.instrument_id(),
        ClientOrderId(kw["customer_order_ref"]),
    )

    # Act
    for raw in orjson.loads(BetfairDataProvider.streaming_ocm_DUPLICATE_EXECUTION()):
        setup_exec_client_and_cache(
            mocker=mocker,
            exec_client=execution_client,
            exec_engine=exec_engine,
            logger=logger,
            raw=orjson.dumps(raw),
        )
        execution_client.handle_order_stream_update(raw=orjson.dumps(raw))
        await asyncio.sleep(0.3)

    # Assert
    events = exec_engine.events
    assert isinstance(events[0], OrderAccepted)
    assert isinstance(events[1], OrderAccepted)
    # First order example, partial fill followed by remainder canceled
    assert isinstance(events[2], OrderFilled)
    assert isinstance(events[3], AccountState)
    assert isinstance(events[4], OrderCanceled)
    # Second order example, partial fill followed by remainder filled
    assert (
        isinstance(events[5], OrderFilled)
        and events[5].execution_id.value == "4721ad7594e7a4a4dffb1bacb0cb45ccdec0747a"
    )
    assert isinstance(events[6], AccountState)
    assert (
        isinstance(events[7], OrderFilled)
        and events[7].execution_id.value == "8b3e65be779968a3fdf2d72731c848c5153e88cd"
    )
    assert isinstance(events[8], AccountState)


@pytest.mark.asyncio
@pytest.mark.skip(reason="Not implemented yet")
async def test_betfair_account_states(execution_client, exec_engine):
    # Setup
    balance = exec_engine.cache.account_for_venue(BETFAIR_VENUE).balances()[AUD]
    expected = {
        "type": "AccountBalance",
        "currency": "AUD",
        "total": "1000.00",
        "locked": "-0.00",
        "free": "1000.00",
    }
    assert balance.to_dict() == expected

    # Create an order to buy at 0.5 ($2.0) for $10 - exposure is $20
    order = BetfairTestStubs.make_order(price=Price.from_str("0.5"), quantity=Quantity.from_int(10))

    # Order accepted - expect balance to drop by exposure
    order_accepted = BetfairTestStubs.event_order_accepted(order=order)
    exec_engine._handle_event(order_accepted)
    await asyncio.sleep(0.1)
    balance = exec_engine.cache.account_for_venue(BETFAIR_VENUE).balances()[AUD]
    expected = {
        "type": "AccountBalance",
        "currency": "AUD",
        "total": "1000.00",
        "locked": "20.00",
        "free": "980.00",
    }
    assert balance.to_dict() == expected

    # Cancel the order, balance should return
    cancelled = BetfairTestStubs.event_order_canceled(order=order)
    exec_engine._handle_event(cancelled)
    await asyncio.sleep(0.1)
    balance = exec_engine.cache.account_for_venue(BETFAIR_VENUE).balances()[AUD]
    expected = {
        "type": "AccountBalance",
        "currency": "AUD",
        "total": "1000.00",
        "locked": "-0.00",
        "free": "1080.00",
    }
    assert balance.to_dict() == expected
