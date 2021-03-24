from functools import lru_cache

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import load_instruments
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.mocks import MockLiveDataEngine
from tests.test_kit.mocks import MockLiveExecutionEngine


load_instruments = lru_cache()(load_instruments)


@pytest.fixture(autouse=True)
def betfairlightweight_mocks(mocker):
    # TODO - Mocks not currently working in TestKit, need to stay here

    # Betfair client login
    mocker.patch("betfairlightweight.endpoints.login.Login.__call__")

    # Navigation.list_navigation
    mock_list_nav = mocker.patch(
        "betfairlightweight.endpoints.navigation.Navigation.list_navigation"
    )
    mock_list_nav.return_value = BetfairTestStubs.resp_navigation()

    # Betting.list_market_catalogue
    mock_market_catalogue = mocker.patch(
        "betfairlightweight.endpoints.betting.Betting.list_market_catalogue"
    )
    mock_market_catalogue.return_value = BetfairTestStubs.resp_market_catalogue()

    # Account.get_account_details
    mock_account_detail = mocker.patch(
        "betfairlightweight.endpoints.account.Account.get_account_details"
    )
    mock_account_detail.return_value = BetfairTestStubs.resp_account_detail()

    # Account.get_account_funds
    mock_account_funds = mocker.patch(
        "betfairlightweight.endpoints.account.Account.get_account_funds"
    )
    mock_account_funds.return_value = BetfairTestStubs.resp_account_funds()

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
    return AccountId(BETFAIR_VENUE.value, "001")


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
    return MockLiveDataEngine(
        loop=event_loop,
        portfolio=portfolio,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
def exec_engine(event_loop, clock, live_logger, portfolio, trader_id):
    database = BypassExecutionDatabase(trader_id=trader_id, logger=live_logger)
    return MockLiveExecutionEngine(
        loop=event_loop,
        database=database,
        portfolio=portfolio,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
def betting_instrument(provider):
    return BetfairTestStubs.betting_instrument()


@pytest.fixture()
def betfair_order_socket(betfair_client):
    return BetfairOrderStreamClient(client=betfair_client, message_handler=None)


@pytest.fixture()
def betfair_market_socket():
    return BetfairMarketStreamClient(client=betfair_client, message_handler=None)


@pytest.fixture()
async def execution_client(
    betfair_client, account_id, exec_engine, clock, live_logger
) -> BetfairExecutionClient:
    client = BetfairExecutionClient(
        client=betfair_client,
        account_id=account_id,
        engine=exec_engine,
        clock=clock,
        logger=live_logger,
    )
    client.instrument_provider().load_all()
    exec_engine.register_client(client)
    return client


@pytest.fixture()
def betfair_data_client(betfair_client, data_engine, clock, live_logger):
    client = BetfairDataClient(
        client=betfair_client,
        engine=data_engine,
        clock=clock,
        logger=live_logger,
    )
    client.instrument_provider().load_all()
    data_engine.register_client(client)
    return client
