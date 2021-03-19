import json

import betfairlightweight
import pytest

from adapters.betfair.common import BETFAIR_VENUE
from adapters.betfair.providers import BetfairInstrumentProvider
from model.identifiers import InstrumentId
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/betfair/responses/"


@pytest.fixture(autouse=True)
def mocks(mocker):
    mock_list_nav = mocker.patch(
        "betfairlightweight.endpoints.navigation.Navigation.list_navigation"
    )
    mock_list_nav.return_value = json.loads(open(TEST_PATH + "navigation.json").read())

    mock_market_catalogue = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_market_catalogue"
    )
    mock_market_catalogue.return_value = json.loads(
        open(TEST_PATH + "market_metadata.json").read()
    )


@pytest.fixture()
def provider(betfair_client) -> BetfairInstrumentProvider:
    return BetfairInstrumentProvider(
        client=betfair_client, market_filter={"event_type_name": "Tennis"}
    )


@pytest.fixture()
def clock():
    return LiveClock()


@pytest.fixture()
def live_logger(clock):
    return LiveLogger(clock)


@pytest.fixture()
def portfolio(clock, live_logger):
    return Portfolio(
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
def trader_id():
    return TraderId("TESTER", "001")


@pytest.fixture()
def account_id():
    return AccountId("Betfair", "001")


@pytest.fixture()
def strategy_id():
    return StrategyId(name="Test", tag="1")


@pytest.fixture()
def position_id():
    return PositionId("1")


@pytest.fixture()
def instrument_id():
    return InstrumentId(symbol=Symbol("Test"), venue=BETFAIR_VENUE)


@pytest.fixture()
def uuid():
    return UUIDFactory().generate()


@pytest.fixture()
def data_engine(event_loop, clock, live_logger, portfolio):
    return LiveDataEngine(
        loop=event_loop,
        portfolio=portfolio,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
@pytest.mark.asyncio()
def exec_engine(event_loop, clock, live_logger, portfolio, trader_id):
    database = BypassExecutionDatabase(trader_id=trader_id, logger=live_logger)
    return LiveExecutionEngine(
        loop=event_loop,
        database=database,
        portfolio=portfolio,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
def betfair_client():
    return betfairlightweight.APIClient(
        username="username",
        password="password",
        app_key="app_key",
        certs="cert_location",
    )


@pytest.fixture()
def execution_client(betfair_client, account_id, exec_engine, clock, live_logger):
    client = BetfairExecutionClient(
        client=betfair_client,
        account_id=account_id,
        engine=exec_engine,
        clock=clock,
        logger=live_logger,
    )
    exec_engine.register_client(client)
    return client


@pytest.fixture()
def betting_instrument(provider):
    return provider.search_instruments(
        instrument_filter={"event_type_name": "American Football"}
    )[0]
