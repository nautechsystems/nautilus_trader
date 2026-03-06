"""
Shared fixtures for unit tests.

These fixtures use TestClock for deterministic timing. For live/integration tests that
need real-time clocks, use TestComponentStubs instead.

"""

import pytest

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


@pytest.fixture(name="clock")
def fixture_clock():
    return TestClock()


@pytest.fixture(name="trader_id")
def fixture_trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture(name="strategy_id")
def fixture_strategy_id():
    return TestIdStubs.strategy_id()


@pytest.fixture(name="account_id")
def fixture_account_id():
    return TestIdStubs.account_id()


@pytest.fixture(name="msgbus")
def fixture_msgbus(trader_id, clock):
    return MessageBus(
        trader_id=trader_id,
        clock=clock,
    )


@pytest.fixture(name="cache")
def fixture_cache():
    cache = Cache()
    cache.add_instrument(AUDUSD_SIM)
    cache.add_instrument(GBPUSD_SIM)
    cache.add_instrument(USDJPY_SIM)
    return cache


@pytest.fixture(name="portfolio")
def fixture_portfolio(msgbus, cache, clock):
    return Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture(name="data_engine")
def fixture_data_engine(msgbus, cache, clock):
    return DataEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture(name="exec_engine")
def fixture_exec_engine(msgbus, cache, clock):
    return ExecutionEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture(name="risk_engine")
def fixture_risk_engine(portfolio, msgbus, cache, clock):
    return RiskEngine(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture(name="strategy")
def fixture_strategy(trader_id, portfolio, msgbus, cache, clock):
    strategy = Strategy()
    strategy.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return strategy
