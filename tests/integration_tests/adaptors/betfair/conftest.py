import betfairlightweight
import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio


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
def data_engine(event_loop, clock, live_logger, portfolio):
    return LiveDataEngine(
        loop=event_loop,
        portfolio=portfolio,
        clock=clock,
        logger=live_logger,
    )


@pytest.fixture()
@pytest.mark.asyncio()
def exec_engine(event_loop, clock, live_logger, portfolio):
    trader_id = TraderId("TESTER", "001")
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
