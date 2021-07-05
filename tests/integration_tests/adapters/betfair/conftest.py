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

import pytest

from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.execution import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.database import BypassCacheDatabase
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.fixture(autouse=True)
def betfairlightweight_mocks(mocker):
    # TODO - Mocks not currently working in TestKit, need to stay here

    # Betfair client login
    mocker.patch("betfairlightweight.endpoints.login.Login.__call__")

    # Mock Navigation / market catalogue endpoints
    mocker.patch(
        "betfairlightweight.endpoints.navigation.Navigation.list_navigation",
        return_value=BetfairTestStubs.navigation(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_market_catalogue",
        return_value=BetfairTestStubs.market_catalogue_short(),
    )

    # Mock Account endpoints
    mocker.patch(
        "betfairlightweight.endpoints.account.Account.get_account_details",
        return_value=BetfairTestStubs.account_detail(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.account.Account.get_account_funds",
        return_value=BetfairTestStubs.account_funds_no_exposure(),
    )

    # Mock Betting endpoints
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.place_orders",
        return_value=BetfairTestStubs.place_orders_success(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.replace_orders",
        return_value=BetfairTestStubs.replace_orders_success(),
    )
    mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.cancel_orders",
        return_value=BetfairTestStubs.cancel_orders_success(),
    )

    # Streaming endpoint
    mocker.patch(
        "nautilus_trader.adapters.betfair.sockets.HOST",
        return_value=BetfairTestStubs.integration_endpoint(),
    )


@pytest.fixture(scope="session")
def betfair_client():
    return BetfairTestStubs.betfair_client()


@pytest.fixture(scope="session")
def provider(betfair_client) -> BetfairInstrumentProvider:
    return BetfairTestStubs.instrument_provider(betfair_client)


@pytest.fixture()
def clock() -> LiveClock:
    return BetfairTestStubs.clock()


@pytest.fixture()
def live_logger(event_loop, clock):
    return LiveLogger(loop=event_loop, clock=clock, level_stdout=LogLevel.INFO)


@pytest.fixture()
def cache_db(trader_id, live_logger):
    return BypassCacheDatabase(
        trader_id=trader_id,
        logger=live_logger,
    )


@pytest.fixture()
def cache(live_logger, cache_db):
    return Cache(
        database=cache_db,
        logger=live_logger,
    )


@pytest.fixture()
def portfolio(cache, clock, live_logger):
    return Portfolio(
        cache=cache,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
def trader_id():
    return BetfairTestStubs.trader_id()


@pytest.fixture()
def account_id():
    return BetfairTestStubs.account_id()


@pytest.fixture()
def strategy_id():
    return BetfairTestStubs.strategy_id()


@pytest.fixture()
def position_id():
    return PositionId("1")


@pytest.fixture()
def instrument_id():
    return InstrumentId(symbol=Symbol("Test"), venue=Venue("BETFAIR"))


@pytest.fixture()
def uuid():
    return UUIDFactory().generate()


@pytest.fixture()
def data_engine(event_loop, clock, live_logger, portfolio):
    return BetfairTestStubs.mock_live_data_engine()


@pytest.fixture()
def exec_engine(event_loop, clock, live_logger, portfolio, trader_id):
    return BetfairTestStubs.mock_live_exec_engine()


@pytest.fixture()
def risk_engine(event_loop, clock, live_logger, portfolio, trader_id):
    return BetfairTestStubs.mock_live_risk_engine()


@pytest.fixture()
def betting_instrument(provider):
    return BetfairTestStubs.betting_instrument()


@pytest.fixture()
def betfair_account_state(betfair_client, uuid):
    details = betfair_client.account.get_account_details()
    funds = betfair_client.account.get_account_funds()
    return betfair_account_to_account_state(
        account_detail=details,
        account_funds=funds,
        event_id=uuid,
        ts_updated_ns=0,
        timestamp_ns=0,
    )


@pytest.fixture()
def betfair_order_socket(betfair_client, live_logger):
    return BetfairOrderStreamClient(client=betfair_client, logger=live_logger, message_handler=None)


@pytest.fixture()
def betfair_market_socket():
    return BetfairMarketStreamClient(
        client=betfair_client, logger=live_logger, message_handler=None
    )


@pytest.fixture()
async def execution_client(
    betfair_client, account_id, exec_engine, clock, live_logger, betfair_account_state
) -> BetfairExecutionClient:
    client = BetfairExecutionClient(
        client=betfair_client,
        account_id=account_id,
        base_currency=AUD,
        engine=exec_engine,
        clock=clock,
        logger=live_logger,
        market_filter={},
        load_instruments=False,
    )
    client.instrument_provider().load_all()
    exec_engine.register_client(client)
    exec_engine.cache.add_account(account=Account(betfair_account_state))
    return client


@pytest.fixture()
def betfair_data_client(betfair_client, data_engine, clock, live_logger):
    client = BetfairDataClient(
        client=betfair_client,
        engine=data_engine,
        clock=clock,
        logger=live_logger,
        market_filter={},
        load_instruments=True,
    )
    data_engine.register_client(client)
    return client
