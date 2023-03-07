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

import pytest
from pytest_mock import MockerFixture

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


@pytest.fixture()
def account_id(venue):
    return AccountId(f"{venue.value}-001")


@pytest.fixture()
def clock():
    return LiveClock()


@pytest.fixture()
def logger(clock):
    return Logger(clock)


@pytest.fixture()
def trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture()
def msgbus(trader_id, clock, logger):
    return MessageBus(
        trader_id,
        clock,
        logger,
    )


@pytest.fixture()
def cache(logger, instrument):
    cache = TestComponentStubs.cache(logger)
    cache.add_instrument(instrument)
    return cache


@pytest.fixture()
def portfolio(clock, logger, cache, msgbus):
    return Portfolio(
        msgbus,
        cache,
        clock,
        logger,
    )


@pytest.fixture()
def data_engine(msgbus, cache, clock, logger, data_client):
    engine = DataEngine(
        msgbus,
        cache,
        clock,
        logger,
    )
    engine.register_client(data_client)
    return engine


@pytest.fixture()
def exec_engine(msgbus, cache, clock, logger, exec_client):
    engine = ExecutionEngine(
        msgbus,
        cache,
        clock,
        logger,
    )
    engine.register_client(exec_client)
    return engine


@pytest.fixture()
def risk_engine(portfolio, msgbus, cache, clock, logger):
    risk_engine = RiskEngine(
        portfolio,
        msgbus,
        cache,
        clock,
        logger,
    )
    return risk_engine


@pytest.fixture(autouse=True)
def trader(
    trader_id,
    msgbus,
    cache,
    portfolio,
    data_engine,
    risk_engine,
    exec_engine,
    clock,
    logger,
    event_loop,
):
    return Trader(
        trader_id=trader_id,
        msgbus=msgbus,
        cache=cache,
        portfolio=portfolio,
        data_engine=data_engine,
        risk_engine=risk_engine,
        exec_engine=exec_engine,
        clock=clock,
        logger=logger,
        loop=event_loop,
    )


@pytest.fixture()
def mock_data_engine_process(mocker: MockerFixture, msgbus, data_engine):
    mock = mocker.MagicMock()
    msgbus.deregister(endpoint="DataEngine.process", handler=data_engine.process)
    msgbus.register(
        endpoint="DataEngine.process",
        handler=mock,
    )
    return mock


@pytest.fixture()
def mock_exec_engine_process(mocker: MockerFixture, msgbus, exec_engine):
    mock = mocker.MagicMock()
    msgbus.deregister(endpoint="ExecEngine.process", handler=exec_engine.process)
    msgbus.register(
        endpoint="ExecEngine.process",
        handler=mock,
    )
    return mock


@pytest.fixture()
def strategy(trader_id, portfolio, msgbus, cache, clock, logger):
    strategy = Strategy()
    strategy.register(
        trader_id,
        portfolio,
        msgbus,
        cache,
        clock,
        logger,
    )
    return strategy


@pytest.fixture()
def strategy_id(strategy):
    return strategy.id


@pytest.fixture()
def client_order_id(strategy):
    return TestIdStubs.client_order_id()


@pytest.fixture()
def venue_order_id(strategy):
    return TestIdStubs.venue_order_id()


@pytest.fixture()
def components(data_engine, exec_engine, risk_engine, strategy):
    return


@pytest.fixture()
def events(msgbus):
    events = []
    msgbus.subscribe("events.*", handler=events.append)
    return events


@pytest.fixture()
def messages(msgbus):
    messages = []
    msgbus.subscribe("*", handler=messages.append)
    return messages


# TO BE IMPLEMENTED IN ADAPTER conftest.py
@pytest.fixture()
def venue() -> Venue:
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def data_client():
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def exec_client():
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def instrument():
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")
