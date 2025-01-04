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
from betfair_parser.exceptions import AccountAPINGException
from betfair_parser.spec.accounts.enums import Wallet
from betfair_parser.spec.accounts.operations import GetAccountDetails
from betfair_parser.spec.accounts.operations import GetAccountFunds
from betfair_parser.spec.accounts.operations import Params
from betfair_parser.spec.accounts.operations import _GetAccountFundsParams
from betfair_parser.spec.accounts.type_definitions import AccountFundsResponse
from betfair_parser.spec.betting.enums import BetStatus
from betfair_parser.spec.betting.enums import PersistenceType
from betfair_parser.spec.betting.listings import ListMarketCatalogue
from betfair_parser.spec.betting.listings import _ListMarketCatalogueParams
from betfair_parser.spec.betting.orders import CancelOrders
from betfair_parser.spec.betting.orders import ListClearedOrders
from betfair_parser.spec.betting.orders import ListCurrentOrders
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.orders import Side
from betfair_parser.spec.betting.orders import _CancelOrdersParams
from betfair_parser.spec.betting.orders import _ListClearedOrdersParams
from betfair_parser.spec.betting.orders import _ListCurrentOrdersParams
from betfair_parser.spec.betting.orders import _PlaceOrdersParams
from betfair_parser.spec.betting.orders import _ReplaceOrdersParams
from betfair_parser.spec.betting.type_definitions import CancelInstruction
from betfair_parser.spec.betting.type_definitions import LimitOrder
from betfair_parser.spec.betting.type_definitions import MarketOnCloseOrder
from betfair_parser.spec.betting.type_definitions import PlaceInstruction
from betfair_parser.spec.betting.type_definitions import ReplaceInstruction
from betfair_parser.spec.common import OrderType
from betfair_parser.spec.common import Response
from betfair_parser.spec.common.messages import RPCError
from betfair_parser.spec.identity import Login
from betfair_parser.spec.identity import _LoginParams
from betfair_parser.spec.navigation import Menu

from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.requests import order_cancel_to_cancel_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_submit_to_place_order_params
from nautilus_trader.adapters.betfair.parsing.requests import order_update_to_replace_order_params
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument_handicap
from tests.integration_tests.adapters.betfair.test_kit import mock_betfair_request


@pytest.mark.asyncio()
async def test_connect(betfair_client):
    # Arrange
    betfair_client.reset_headers()

    # Act
    await betfair_client.connect()
    assert betfair_client._headers["X-Authentication"]

    # Assert
    _, request = betfair_client._request.call_args[0]
    expected = Login(
        jsonrpc="2.0",
        id=request.id,
        params=_LoginParams(username="", password=""),
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_exception_handling(betfair_client):
    mock_betfair_request(betfair_client, response=BetfairResponses.account_funds_error())
    with pytest.raises(AccountAPINGException) as e:
        await betfair_client.get_account_funds(wallet="not a real walltet")
        result = e.value.response
        expected = Response(
            jsonrpc="2.0",
            id=1,
            result=None,
            error=RPCError(code=-32602, message="DSC-0018", data=None),
        )
        assert result == expected


@pytest.mark.asyncio()
async def test_list_navigation(betfair_client):
    mock_betfair_request(betfair_client, BetfairResponses.navigation_list_navigation())
    nav = await betfair_client.list_navigation()
    assert len(nav.children) == 28

    _, request = betfair_client._request.call_args[0]
    assert request == Menu(jsonrpc="2.0", id=0, params=None)


@pytest.mark.asyncio()
async def test_list_market_catalogue(betfair_client):
    market_filter = {
        "eventTypeIds": ["7"],
        "marketBettingTypes": ["ODDS"],
    }
    mock_betfair_request(betfair_client, BetfairResponses.betting_list_market_catalogue())
    catalogue = await betfair_client.list_market_catalogue(filter_=market_filter)
    assert catalogue
    _, request = betfair_client._request.call_args[0]
    expected = ListMarketCatalogue(
        params=_ListMarketCatalogueParams(
            filter={"eventTypeIds": ["7"], "marketBettingTypes": ["ODDS"]},
            market_projection=None,
            sort=None,
            max_results=1000,
            locale=None,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_get_account_details(betfair_client):
    account = await betfair_client.get_account_details()

    assert account.points_balance == 10
    _, request = betfair_client._request.call_args[0]
    expected = GetAccountDetails(params=Params(), id=request.id)
    assert request == expected


@pytest.mark.asyncio()
async def test_get_account_funds(betfair_client):
    mock_betfair_request(betfair_client, BetfairResponses.account_funds_no_exposure())
    response = await betfair_client.get_account_funds()
    _, request = betfair_client._request.call_args[0]
    assert request == GetAccountFunds(
        params=_GetAccountFundsParams(wallet=None),
        id=request.id,
    )
    assert response == AccountFundsResponse(
        available_to_bet_balance=1000.0,
        exposure=0.0,
        retained_commission=0.0,
        exposure_limit=-15000.0,
        discount_rate=0.0,
        points_balance=10,
        wallet=Wallet.UK,
    )


@pytest.mark.asyncio()
async def test_place_orders(betfair_client):
    instrument = betting_instrument()
    limit_order = TestExecStubs.limit_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
        price=betfair_float_to_price(2.0),
        quantity=betfair_float_to_quantity(10),
    )
    command = TestCommandStubs.submit_order_command(order=limit_order)
    place_orders = order_submit_to_place_order_params(command=command, instrument=instrument)
    mock_betfair_request(betfair_client, BetfairResponses.betting_place_order_success())

    await betfair_client.place_orders(place_orders)

    _, request = betfair_client._request.call_args[0]
    expected = PlaceOrders(
        params=_PlaceOrdersParams(
            market_id="1-179082386",
            instructions=[
                PlaceInstruction(
                    order_type=OrderType.LIMIT,
                    selection_id=50214,
                    handicap=None,
                    side=Side.BACK,
                    limit_order=LimitOrder(
                        price=2.0,
                        size=10.0,
                        persistence_type=PersistenceType.PERSIST,
                    ),
                    limit_on_close_order=None,
                    market_on_close_order=None,
                    customer_order_ref="O-20210410-022422-001-001-1",
                ),
            ],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            customer_strategy_ref="4827311aa8c4c74",
            async_=False,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_place_orders_handicap(betfair_client):
    instrument = betting_instrument_handicap()
    limit_order = TestExecStubs.limit_order(
        instrument=instrument,
        order_side=OrderSide.BUY,
        price=betfair_float_to_price(2.0),
        quantity=betfair_float_to_quantity(10.0),
    )
    command = TestCommandStubs.submit_order_command(order=limit_order)
    place_orders = order_submit_to_place_order_params(command=command, instrument=instrument)
    mock_betfair_request(betfair_client, BetfairResponses.betting_place_order_success())

    await betfair_client.place_orders(place_orders)

    _, request = betfair_client._request.call_args[0]
    expected = PlaceOrders(
        params=_PlaceOrdersParams(
            market_id="1-186249896",
            instructions=[
                PlaceInstruction(
                    order_type=OrderType.LIMIT,
                    selection_id=5304641,
                    handicap=-5.5,
                    side=Side.LAY,
                    limit_order=LimitOrder(
                        price=2.0,
                        size=10.0,
                        persistence_type=PersistenceType.PERSIST,
                    ),
                    limit_on_close_order=None,
                    market_on_close_order=None,
                    customer_order_ref="O-20210410-022422-001-001-1",
                ),
            ],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            customer_strategy_ref="4827311aa8c4c74",
            async_=False,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_place_orders_market_on_close(betfair_client):
    instrument = betting_instrument()
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
        command_id=UUID4.from_str("2d89666b-1a1e-4a75-b193-4eb3b454c757"),
        ts_init=0,
    )
    place_orders = order_submit_to_place_order_params(
        command=submit_order_command,
        instrument=instrument,
    )
    mock_betfair_request(betfair_client, BetfairResponses.betting_place_order_success())

    resp = await betfair_client.place_orders(place_orders)
    assert resp

    _, request = betfair_client._request.call_args[0]
    expected = PlaceOrders(
        params=_PlaceOrdersParams(
            market_id="1-179082386",
            instructions=[
                PlaceInstruction(
                    order_type=OrderType.MARKET_ON_CLOSE,
                    selection_id=50214,
                    handicap=None,
                    side=Side.LAY,
                    limit_order=None,
                    limit_on_close_order=None,
                    market_on_close_order=MarketOnCloseOrder(liability=10.0),
                    customer_order_ref="O-20210410-022422-001-001-1",
                ),
            ],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            customer_strategy_ref="4827311aa8c4c74",
            async_=False,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_replace_orders_single(betfair_client):
    instrument = betting_instrument()
    update_order_command = TestCommandStubs.modify_order_command(
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("1628717246480-1.186260932-rpl-0"),
        price=betfair_float_to_price(2.0),
    )
    replace_order = order_update_to_replace_order_params(
        command=update_order_command,
        venue_order_id=VenueOrderId("240718603398"),
        instrument=instrument,
    )
    mock_betfair_request(betfair_client, BetfairResponses.betting_replace_orders_success())

    resp = await betfair_client.replace_orders(replace_order)
    assert resp

    _, request = betfair_client._request.call_args[0]
    expected = ReplaceOrders(
        params=_ReplaceOrdersParams(
            market_id="1-179082386",
            instructions=[ReplaceInstruction(bet_id=240718603398, new_price=2.0)],
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            market_version=None,
            async_=False,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_cancel_orders(betfair_client):
    instrument = betting_instrument()
    cancel_command = TestCommandStubs.cancel_order_command(
        venue_order_id=VenueOrderId("228302937743"),
    )
    cancel_order_params = order_cancel_to_cancel_order_params(
        command=cancel_command,
        instrument=instrument,
    )
    mock_betfair_request(betfair_client, BetfairResponses.betting_cancel_orders_success())

    resp = await betfair_client.cancel_orders(cancel_order_params)
    assert resp

    _, request = betfair_client._request.call_args[0]
    expected = CancelOrders(
        params=_CancelOrdersParams(
            market_id="1-179082386",
            customer_ref="2d89666b1a1e4a75b1934eb3b454c757",
            instructions=[CancelInstruction(bet_id=228302937743)],
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_list_current_orders(betfair_client):
    mock_betfair_request(betfair_client, response=BetfairResponses.list_current_orders_executable())
    current_orders = await betfair_client.list_current_orders()
    assert len(current_orders) == 2

    _, request = betfair_client._request.call_args[0]
    expected = ListCurrentOrders(
        params=_ListCurrentOrdersParams(
            bet_ids=None,
            market_ids=None,
            order_projection=None,
            customer_order_refs=None,
            customer_strategy_refs=None,
            date_range=None,
            order_by=None,
            sort_dir=None,
            from_record=0,
            record_count=None,
            include_item_description=None,
        ),
        id=request.id,
    )
    assert request == expected


@pytest.mark.asyncio()
async def test_list_cleared_orders(betfair_client):
    mock_betfair_request(betfair_client, response=BetfairResponses.list_cleared_orders())
    cleared_orders = await betfair_client.list_cleared_orders(bet_status=BetStatus.SETTLED)
    assert len(cleared_orders) == 14

    _, request = betfair_client._request.call_args[0]
    expected = ListClearedOrders(
        params=_ListClearedOrdersParams(
            bet_status=BetStatus.SETTLED,
            event_type_ids=None,
            event_ids=None,
            market_ids=None,
            runner_ids=None,
            bet_ids=None,
            customer_order_refs=None,
            customer_strategy_refs=None,
            side=None,
            settled_date_range=None,
            group_by=None,
            include_item_description=None,
            locale=None,
            from_record=0,
            record_count=None,
        ),
        id=request.id,
    )
    assert request == expected
