import asyncio
import sys

import pytest

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.client.exceptions import BetfairAPIError
from nautilus_trader.adapters.betfair.parsing import order_cancel_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_submit_to_betfair
from nautilus_trader.adapters.betfair.parsing import order_update_to_betfair
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
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
from tests.integration_tests.adapters.betfair.test_kit import mock_client_request
from tests.test_kit.stubs import TestStubs


pytestmark = pytest.mark.skipif(sys.version_info < (3, 8), reason="Mock client broken on 3.7")


class TestBetfairClient:
    def setup(self):
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = LiveLogger(loop=self.loop, clock=self.clock)
        self.client = BetfairClient(  # noqa: S106
            username="username",
            password="password",
            app_key="app_key",
            cert_dir="/certs",
            loop=self.loop,
            logger=self.logger,
            ssl=True,
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
        with mock_client_request(response=BetfairResponses.account_funds_error()["result"]):
            with pytest.raises(BetfairAPIError) as e:
                await self.client.get_account_funds(wallet="not a real walltet")
            assert e.value.message == "DSC-0018"

    @pytest.mark.asyncio
    async def test_list_navigation(self):
        with mock_client_request(
            response=BetfairResponses.navigation_list_navigation()
        ) as mock_request:
            nav = await self.client.list_navigation()
            assert len(nav["children"]) == 28

        result = mock_request.call_args.kwargs
        expected = BetfairRequests.navigation_list_navigation()
        assert result == expected

    @pytest.mark.asyncio
    async def test_list_market_catalogue(self):
        market_filter = {
            "eventTypeIds": ["7"],
            "marketBettingTypes": ["ODDS"],
        }
        with mock_client_request(
            response=BetfairResponses.betting_list_market_catalogue()
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
            response=BetfairResponses.account_funds_no_exposure()
        ) as mock_request:
            funds = await self.client.get_account_funds()
            assert funds["availableToBetBalance"] == 1000.0
        result = mock_request.call_args.kwargs
        expected = BetfairRequests.account_funds()
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders_handicap(self):
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
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            await self.client.place_orders(**place_orders)

        expected = BetfairRequests.betting_place_order_handicap()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders(self):
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
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            await self.client.place_orders(**place_orders)

        expected = BetfairRequests.betting_place_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_place_orders_market_on_close(self):
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
        with mock_client_request(response=BetfairResponses.betting_place_order_success()) as req:
            resp = await self.client.place_orders(**place_orders)
            assert resp

        expected = BetfairRequests.betting_place_order_bsp()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_replace_orders_single(self):
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
        with mock_client_request(response=BetfairResponses.betting_replace_orders_success()) as req:
            resp = await self.client.replace_orders(**replace_order)
            assert resp

        expected = BetfairRequests.betting_replace_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_replace_orders_multi(self):
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
        with mock_client_request(
            response=BetfairResponses.betting_replace_orders_success_multi()
        ) as req:
            resp = await self.client.replace_orders(**replace_order)
            assert len(resp["oc"][0]["orc"][0]["uo"]) == 2

        expected = BetfairRequests.betting_replace_order()
        result = req.call_args.kwargs["json"]
        assert result == expected

    @pytest.mark.asyncio
    async def test_cancel_orders(self):
        instrument = BetfairTestStubs.betting_instrument()
        cancel_command = BetfairTestStubs.cancel_order_command()
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
