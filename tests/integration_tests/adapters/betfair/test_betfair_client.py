# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from ssl import SSLContext

import msgspec.json
import pytest

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.client.exceptions import BetfairAPIError
from nautilus_trader.adapters.betfair.client.spec import BetfairSide
from nautilus_trader.adapters.betfair.client.spec import BetOutcome
from nautilus_trader.adapters.betfair.client.spec import ClearedOrder
from nautilus_trader.adapters.betfair.client.spec import ClearedOrdersResponse
from nautilus_trader.adapters.betfair.client.spec import OrderType
from nautilus_trader.adapters.betfair.client.spec import PersistenceType
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.requests import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing.requests import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_betfair
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairRequests
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import mock_client_request


class TestBetfairClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock, bypass=True)
        self.client = BetfairClient(  # noqa: S106 (no hardcoded password)
            username="username",
            password="password",
            app_key="app_key",
            cert_dir="/certs",
            loop=self.loop,
            logger=self.logger,
            ssl=SSLContext(),
        )
        self.client.session_token = "xxxsessionToken="

    @pytest.mark.asyncio
    async def test_connect(self):
        self.client.session_token = None
        with mock_client_request(response=BetfairResponses.cert_login()) as mock_request:
            await self.client.connect()
            assert self.client.session_token

        result = mock_request.call_args.kwargs
        expected = BetfairRequests.cert_login()
        assert result == expected

    @pytest.mark.asyncio
    async def test_exception_handling(self):
        with mock_client_request(response=BetfairResponses.account_funds_error()):
            with pytest.raises(BetfairAPIError) as e:
                await self.client.get_account_funds(wallet="not a real walltet")
            assert e.value.message == "DSC-0018"

    @pytest.mark.asyncio
    async def test_list_navigation(self):
        with mock_client_request(
            response=BetfairResponses.navigation_list_navigation_response(),
        ) as mock_request:
            nav = await self.client.list_navigation()
            assert len(nav.children) == 28

        result = mock_request.call_args.kwargs
        expected = BetfairRequests.navigation_list_navigation_request()
        assert result == expected

    @pytest.mark.asyncio
    async def test_list_market_catalogue(self):
        market_filter = {
            "eventTypeIds": ["7"],
            "marketBettingTypes": ["ODDS"],
        }
        with mock_client_request(
            response=BetfairResponses.betting_list_market_catalogue(),
        ) as mock_request:
            catalogue = await self.client.list_market_catalogue(filter_=market_filter)
            assert catalogue
        result = mock_request.call_args.kwargs
        expected = BetfairRequests.betting_list_market_catalogue()
        assert result == expected

    @pytest.mark.asyncio
    async def test_get_account_details(self):
        with mock_client_request(response=BetfairResponses.account_details()) as mock_request:
            account = await self.client.get_account_details()

        assert account["pointsBalance"] == 10
        result = mock_request.call_args.kwargs
        expected = BetfairRequests.account_details()
        assert result == expected

    @pytest.mark.asyncio
    async def test_get_account_funds(self):
        with mock_client_request(
            response=BetfairResponses.account_funds_no_exposure(),
        ) as mock_request:
            funds = await self.client.get_account_funds()
            assert funds["availableToBetBalance"] == 1000.0
        result = mock_request.call_args.kwargs
        expected = BetfairRequests.account_funds()
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders_handicap(self):
        instrument = TestInstrumentProvider.betting_instrument_handicap()
        limit_order = TestExecStubs.limit_order(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            price=betfair_float_to_price(2.0),
            quantity=betfair_float_to_quantity(10.0),
        )
        command = TestCommandStubs.submit_order_command(order=limit_order)
        place_orders = order_submit_to_betfair(command=command, instrument=instrument)
        place_orders["instructions"][0]["customerOrderRef"] = "O-20210811-112151-000"
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            await self.client.place_orders(**place_orders)

        expected = BetfairRequests.betting_place_order_handicap()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders(self):
        instrument = TestInstrumentProvider.betting_instrument()
        limit_order = TestExecStubs.limit_order(
            instrument_id=instrument.id,
            order_side=OrderSide.BUY,
            price=betfair_float_to_price(2.0),
            quantity=betfair_float_to_quantity(10),
        )
        command = TestCommandStubs.submit_order_command(order=limit_order)
        place_orders = order_submit_to_betfair(command=command, instrument=instrument)
        place_orders["instructions"][0]["customerOrderRef"] = "O-20210811-112151-000"
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            await self.client.place_orders(**place_orders)

        expected = BetfairRequests.betting_place_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders_market_on_close(self):
        instrument = TestInstrumentProvider.betting_instrument()
        market_on_close_order = TestExecStubs.market_order(
            order_side=OrderSide.BUY,
            time_in_force=TimeInForce.AT_THE_OPEN,
            quantity=betfair_float_to_quantity(10.0),
        )
        submit_order_command = SubmitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            position_id=PositionId("1"),
            order=market_on_close_order,
            command_id=UUID4("be7dffa0-46f2-fce5-d820-c7634d022ca1"),
            ts_init=0,
        )
        place_orders = order_submit_to_betfair(command=submit_order_command, instrument=instrument)
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            resp = await self.client.place_orders(**place_orders)
            assert resp

        expected = BetfairRequests.betting_place_order_bsp()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_replace_orders_single(self):
        instrument = TestInstrumentProvider.betting_instrument()
        update_order_command = TestCommandStubs.modify_order_command(
            instrument_id=instrument.id,
            client_order_id=ClientOrderId("1628717246480-1.186260932-rpl-0"),
            price=betfair_float_to_price(2.0),
        )
        replace_order = order_update_to_betfair(
            command=update_order_command,
            venue_order_id=VenueOrderId("240718603398"),
            side=OrderSide.BUY,
            instrument=instrument,
        )
        with mock_client_request(response=BetfairResponses.betting_replace_orders_success()) as req:
            resp = await self.client.replace_orders(**replace_order)
            assert resp

        expected = BetfairRequests.betting_replace_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_replace_orders_multi(self):
        instrument = TestInstrumentProvider.betting_instrument()
        update_order_command = TestCommandStubs.modify_order_command(
            instrument_id=instrument.id,
            price=betfair_float_to_price(2.0),
            client_order_id=ClientOrderId("1628717246480-1.186260932-rpl-0"),
        )
        replace_order = order_update_to_betfair(
            command=update_order_command,
            venue_order_id=VenueOrderId("240718603398"),
            side=OrderSide.BUY,
            instrument=instrument,
        )
        with mock_client_request(
            response=BetfairResponses.betting_replace_orders_success_multi(),
        ) as req:
            resp = await self.client.replace_orders(**replace_order)
            assert len(resp["oc"][0]["orc"][0]["uo"]) == 2

        expected = BetfairRequests.betting_replace_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_cancel_orders(self):
        instrument = TestInstrumentProvider.betting_instrument()
        cancel_command = TestCommandStubs.cancel_order_command(
            venue_order_id=VenueOrderId("228302937743"),
        )
        cancel_order = order_cancel_to_betfair(command=cancel_command, instrument=instrument)
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            resp = await self.client.cancel_orders(**cancel_order)
            assert resp

        expected = BetfairRequests.betting_cancel_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_list_current_orders(self):
        with mock_client_request(response=BetfairResponses.list_current_orders()) as req:
            current_orders = await self.client.list_current_orders()
            assert len(current_orders) == 4

        expected = {
            "id": 1,
            "jsonrpc": "2.0",
            "method": "SportsAPING/v1.0/listCurrentOrders",
            "params": {"fromRecord": 0, "orderBy": "BY_PLACE_TIME"},
        }
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_list_cleared_orders(self):
        with mock_client_request(response=BetfairResponses.list_cleared_orders()) as req:
            cleared_orders = await self.client.list_cleared_orders()
            assert len(cleared_orders) == 14

        expected = {
            "id": 1,
            "jsonrpc": "2.0",
            "method": "SportsAPING/v1.0/listClearedOrders",
            "params": {"betStatus": "SETTLED", "fromRecord": 0},
        }
        result = req.call_args.kwargs["json"]
        assert result == expected

    def test_api_error(self):
        ex = BetfairAPIError(code="404", message="new error")
        assert (
            str(ex)
            == "BetfairAPIError(code='404', message='new error', kind='None', reason='None')"
        )


class TestBetfairClientSpec:
    def test_cleared_orders(self):
        data = BetfairResponses.list_cleared_orders()["result"]
        raw = msgspec.json.encode(data)
        result = msgspec.json.decode(raw, type=ClearedOrdersResponse)
        expected = ClearedOrdersResponse(
            clearedOrders=[
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226125004209",
                    placedDate="2021-03-05T02:02:30.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=3.85,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T02:02:35.000Z",
                    betCount=1,
                    priceMatched=5.31,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-4b0eb05ab8bf450885fa3df0",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226125004212",
                    placedDate="2021-03-05T02:02:30.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=3.85,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T02:02:35.000Z",
                    betCount=1,
                    priceMatched=5.3,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-f51bd68417774e989731604e",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226125004213",
                    placedDate="2021-03-05T02:02:30.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=3.85,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T02:02:35.000Z",
                    betCount=1,
                    priceMatched=5.3,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-cf5fd124cc8e4a08a55a86ed",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226127299089",
                    placedDate="2021-03-05T03:20:50.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.LAY,
                    betOutcome=BetOutcome.WON,
                    priceRequested=1.5,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T03:21:03.000Z",
                    betCount=1,
                    priceMatched=1.49,
                    priceReduced=False,
                    sizeSettled=250.0,
                    profit=250.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226127312595",
                    placedDate="2021-03-05T03:21:18.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.LAY,
                    betOutcome=BetOutcome.WON,
                    priceRequested=1.55,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T03:21:34.000Z",
                    betCount=1,
                    priceMatched=1.55,
                    priceReduced=False,
                    sizeSettled=50.0,
                    profit=50.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30324808",
                    marketId="1.180076044",
                    selectionId=237491,
                    handicap=0.0,
                    betId="226127366823",
                    placedDate="2021-03-05T03:23:24.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=1.66,
                    settledDate="2021-03-05T03:31:18.000Z",
                    lastMatchedDate="2021-03-05T03:23:36.000Z",
                    betCount=1,
                    priceMatched=1.66,
                    priceReduced=False,
                    sizeSettled=50.0,
                    profit=-50.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30321505",
                    marketId="1.179946477",
                    selectionId=1196397,
                    handicap=0.0,
                    betId="226066832913",
                    placedDate="2021-03-04T10:05:25.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=1.79,
                    settledDate="2021-03-04T10:22:31.000Z",
                    lastMatchedDate="2021-03-04T10:05:30.000Z",
                    betCount=1,
                    priceMatched=5.08,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-38e6bdca26c54a9c9b4a7580",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30321505",
                    marketId="1.179946477",
                    selectionId=1196397,
                    handicap=0.0,
                    betId="226066861797",
                    placedDate="2021-03-04T10:06:06.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=5.9,
                    settledDate="2021-03-04T10:22:31.000Z",
                    lastMatchedDate="2021-03-04T10:06:11.000Z",
                    betCount=1,
                    priceMatched=6.4,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-72142c35871340b2bb84536e",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30321505",
                    marketId="1.179946477",
                    selectionId=1196397,
                    handicap=0.0,
                    betId="226066866254",
                    placedDate="2021-03-04T10:06:12.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=3.05,
                    settledDate="2021-03-04T10:22:31.000Z",
                    lastMatchedDate="2021-03-04T10:06:18.000Z",
                    betCount=1,
                    priceMatched=12.0,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef="betfair-9d78d7f9e3ca4985be1bab14",
                    customerStrategyRef="betfair",
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30249758",
                    marketId="1.178370863",
                    selectionId=237480,
                    handicap=0.0,
                    betId="222629165302",
                    placedDate="2021-01-25T04:40:20.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.WON,
                    priceRequested=1.07,
                    settledDate="2021-01-25T05:31:24.000Z",
                    lastMatchedDate="2021-01-25T04:40:25.000Z",
                    betCount=1,
                    priceMatched=1.08,
                    priceReduced=False,
                    sizeSettled=10.0,
                    profit=0.8,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="7522",
                    eventId="30249758",
                    marketId="1.178370863",
                    selectionId=237480,
                    handicap=0.0,
                    betId="222629517851",
                    placedDate="2021-01-25T04:58:55.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.LAY,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=1.06,
                    settledDate="2021-01-25T05:31:24.000Z",
                    lastMatchedDate="2021-01-25T04:59:00.000Z",
                    betCount=1,
                    priceMatched=1.03,
                    priceReduced=False,
                    sizeSettled=10.2,
                    profit=-0.31,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="4",
                    eventId="30208329",
                    marketId="1.177412197",
                    selectionId=37302,
                    handicap=0.0,
                    betId="221484674007",
                    placedDate="2021-01-11T04:58:28.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=150.0,
                    settledDate="2021-01-11T07:15:11.000Z",
                    lastMatchedDate="2021-01-11T04:58:33.000Z",
                    betCount=1,
                    priceMatched=150.0,
                    priceReduced=False,
                    sizeSettled=10.0,
                    profit=-10.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="4",
                    eventId="30208329",
                    marketId="1.177412195",
                    selectionId=60443,
                    handicap=0.0,
                    betId="221484779119",
                    placedDate="2021-01-11T05:04:01.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.LIMIT,
                    side=BetfairSide.LAY,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=2.02,
                    settledDate="2021-01-11T07:12:30.000Z",
                    lastMatchedDate="2021-01-11T05:04:06.000Z",
                    betCount=1,
                    priceMatched=2.0,
                    priceReduced=False,
                    sizeSettled=20.0,
                    profit=-20.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
                ClearedOrder(
                    eventTypeId="7",
                    eventId="30782786",
                    marketId="1.186277983",
                    selectionId=28563755,
                    handicap=0.0,
                    betId="240718820558",
                    placedDate="2021-08-11T21:35:34.000Z",
                    persistenceType=PersistenceType.LAPSE,
                    orderType=OrderType.MARKET_AT_THE_CLOSE,
                    side=BetfairSide.BACK,
                    betOutcome=BetOutcome.LOST,
                    priceRequested=2.79,
                    settledDate="2021-08-11T21:44:42.000Z",
                    lastMatchedDate="2021-08-11T21:39:10.000Z",
                    betCount=1,
                    priceMatched=2.79,
                    priceReduced=False,
                    sizeSettled=5.0,
                    profit=-5.0,
                    customerOrderRef=None,
                    customerStrategyRef=None,
                ),
            ],
            moreAvailable=False,
        )
        assert result == expected
