import os
from unittest import mock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.betfair.client import BetfairClient
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.core.uuid import UUID
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.betfair.test_kit import BetfairRequests
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


@pytest.fixture()
def client(event_loop, live_logger) -> BetfairClient:
    client = BetfairClient(  # noqa: S106
        username="username",  # os.environ["BETFAIR_USERNAME"],
        password="password",  # os.environ["BETFAIR_PASSWORD"],
        app_key="app_key",  # os.environ["BETFAIR_APP_KEY"],
        cert_dir=os.environ["BETFAIR_CERT_DIR"],
        loop=event_loop,
        logger=live_logger,
    )
    client.session_token = "xxxsessionToken="
    return client


# TODO - Used to get data for test request/responses
# @pytest.fixture()
# def client2():
#     client = APIClient(
#         username=os.environ["BETFAIR_USERNAME"],
#         password=os.environ["BETFAIR_PASSWORD"],
#         app_key=os.environ["BETFAIR_APP_KEY"],
#         certs=os.environ["BETFAIR_CERT_DIR"],
#     )
#     client.login()
#     return client
#
#
# @pytest.mark.vcr(record_mode="always")
# def test_bflw(client2: APIClient):
#     data = client2.betting.list_market_catalogue(
#         filter=market_filter(
#             event_type_ids=["7"],
#             market_betting_types=["ODDS", "ASIAN_HANDICAP_DOUBLE_LINE"],
#             market_type_codes=["MATCH_ODDS"],
#         ),
#         market_projection=["EVENT_TYPE", "RUNNER_METADATA"],
#         sort="MAXIMUM_TRADED",
#         max_results=1000,
#     )
#     print(data)


@pytest.mark.asyncio
@pytest.mark.skip(reason="local/manual testing only")
async def test_live(event_loop, live_logger):
    client = BetfairClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PASSWORD"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        cert_dir=os.environ["BETFAIR_CERT_DIR"],
        loop=event_loop,
        logger=live_logger,
    )
    await client.connect()
    funds = await client.get_account_funds()
    assert funds


@pytest.mark.asyncio
async def test_connect(client: BetfairClient):
    client.session_token = None
    with mock.patch.object(
        BetfairClient, "request", return_value=BetfairResponses.cert_login()
    ) as mock_request:
        await client.connect()
        assert client.session_token

    result = mock_request.call_args.kwargs
    expected = BetfairRequests.cert_login()
    assert result == expected


# @pytest.mark.asyncio
# async def test_exception_handling(client: BetfairClient):
#     resp = await client.get_account_funds(wallet='not a real walltet')


@pytest.mark.asyncio
async def test_list_navigation(client: BetfairClient):
    with mock.patch.object(
        BetfairClient, "request", return_value=BetfairResponses.navigation_list_navigation()
    ) as mock_request:
        nav = await client.list_navigation()
        assert len(nav["children"]) == 28

    result = mock_request.call_args.kwargs
    expected = BetfairRequests.navigation_list_navigation()
    assert result == expected


@pytest.mark.asyncio
async def test_list_market_catalogue(client: BetfairClient):
    market_filter = {
        "eventTypeIds": ["7"],
        "marketBettingTypes": ["ODDS"],
    }
    with mock.patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_list_market_catalogue()
    ) as mock_request:
        catalogue = await client.list_market_catalogue(market_filter=market_filter)
        assert catalogue
    result = mock_request.call_args.kwargs
    expected = BetfairRequests.betting_list_market_catalogue()
    assert result == expected


@pytest.mark.asyncio
async def test_get_account_details(client: BetfairClient):
    with mock.patch.object(
        BetfairClient, "request", return_value=BetfairResponses.account_details()
    ) as mock_request:
        account = await client.get_account_details()
        assert account["pointsBalance"] == 10

    result = mock_request.call_args.kwargs
    expected = BetfairRequests.account_details()
    assert result == expected


@pytest.mark.asyncio
async def test_get_account_funds(client: BetfairClient):
    with mock.patch.object(
        BetfairClient, "request", return_value=BetfairResponses.account_funds_no_exposure()
    ) as mock_request:
        funds = await client.get_account_funds()
        assert funds["availableToBetBalance"] == 1000.0
    result = mock_request.call_args.kwargs
    expected = BetfairRequests.account_funds()
    assert result == expected


@pytest.mark.asyncio
async def test_place_orders_handicap(client: BetfairClient):
    instrument = BetfairTestStubs.betting_instrument_handicap()
    limit_order = TestStubs.limit_order(
        instrument_id=instrument.id,
        side=OrderSide.BUY,
        price=Price.from_str("0.50"),
        quantity=Quantity.from_int(10),
    )
    command = BetfairTestStubs.submit_order_command(order=limit_order)
    place_orders = order_submit_to_betfair(command=command, instrument=instrument)
    place_orders["instructions"][0]["customerOrderRef"] = "O-20210811-112151-000"
    with patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_place_order_success()
    ) as req:
        await client.place_orders(**place_orders)

    expected = BetfairRequests.betting_place_order_handicap()
    result = req.call_args.kwargs["json"]
    assert result == expected


@pytest.mark.asyncio
async def test_place_orders(client: BetfairClient):
    instrument = BetfairTestStubs.betting_instrument()
    limit_order = TestStubs.limit_order(
        instrument_id=instrument.id,
        side=OrderSide.BUY,
        price=Price.from_str("0.50"),
        quantity=Quantity.from_int(10),
    )
    command = BetfairTestStubs.submit_order_command(order=limit_order)
    place_orders = order_submit_to_betfair(command=command, instrument=instrument)
    place_orders["instructions"][0]["customerOrderRef"] = "O-20210811-112151-000"
    with patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_place_order_success()
    ) as req:
        await client.place_orders(**place_orders)

    expected = BetfairRequests.betting_place_order()
    result = req.call_args.kwargs["json"]
    assert result == expected


@pytest.mark.asyncio
async def test_place_orders_market_on_close(client: BetfairClient):
    instrument = BetfairTestStubs.betting_instrument()
    market_on_close_order = BetfairTestStubs.market_order(
        side=OrderSide.BUY,
        time_in_force=TimeInForce.OC,
    )
    submit_order_command = SubmitOrder(
        trader_id=TestStubs.trader_id(),
        strategy_id=TestStubs.strategy_id(),
        position_id=PositionId("1"),
        order=market_on_close_order,
        command_id=UUID.from_str("be7dffa046f2fce5d820c7634d022ca1"),
        ts_init=0,
    )
    place_orders = order_submit_to_betfair(command=submit_order_command, instrument=instrument)
    with patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_place_order_success()
    ) as req:
        resp = await client.place_orders(**place_orders)
        assert resp

    expected = BetfairRequests.betting_place_order_bsp()
    result = req.call_args.kwargs["json"]
    assert result == expected


@pytest.mark.asyncio
async def test_replace_orders(client: BetfairClient):
    instrument = BetfairTestStubs.betting_instrument()
    update_order_command = BetfairTestStubs.update_order_command(
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("1628717246480-1.186260932-rpl-0"),
    )
    replace_order = order_update_to_betfair(
        command=update_order_command,
        venue_order_id=VenueOrderId("240718603398"),
        side=OrderSide.BUY,
        instrument=instrument,
    )

    with patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_place_order_success()
    ) as req:
        resp = await client.replace_orders(**replace_order)
        assert resp

    expected = BetfairRequests.betting_replace_order()
    result = req.call_args.kwargs["json"]
    assert result == expected


@pytest.mark.asyncio
async def test_cancel_orders(client: BetfairClient):
    instrument = BetfairTestStubs.betting_instrument()
    cancel_command = BetfairTestStubs.cancel_order_command()
    cancel_order = order_cancel_to_betfair(command=cancel_command, instrument=instrument)
    with patch.object(
        BetfairClient, "request", return_value=BetfairResponses.betting_place_order_success()
    ) as req:
        resp = await client.cancel_orders(**cancel_order)
        assert resp

    expected = BetfairRequests.betting_cancel_order()
    result = req.call_args.kwargs["json"]
    assert result == expected
