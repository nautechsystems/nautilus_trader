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

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


@pytest.fixture(scope="function")
def account_id(venue):
    return AccountId(f"{venue.value}-001")


@pytest.fixture(scope="function")
def clock():
    return LiveClock()


@pytest.fixture(scope="function")
def logger(clock):
    return Logger(clock)


@pytest.fixture(scope="function")
def trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture(scope="function")
def msgbus(trader_id, clock, logger):
    return MessageBus(
        trader_id,
        clock,
        logger,
    )


@pytest.fixture(scope="function")
def cache(logger):
    return TestComponentStubs.cache(logger)


@pytest.fixture(scope="function")
def portfolio(clock, logger, cache, msgbus):
    return Portfolio(
        msgbus,
        cache,
        clock,
        logger,
    )


@pytest.fixture(scope="function")
def data_engine(msgbus, cache, clock, logger, data_client):
    engine = DataEngine(
        msgbus,
        cache,
        clock,
        logger,
    )
    engine.register_client(data_client)
    return engine


@pytest.fixture(scope="function")
def exec_engine(msgbus, cache, clock, logger, exec_client):
    engine = ExecutionEngine(
        msgbus,
        cache,
        clock,
        logger,
    )
    engine.register_client(exec_client)
    return engine


@pytest.fixture(scope="function")
def risk_engine(portfolio, msgbus, cache, clock, logger):
    return RiskEngine(
        portfolio,
        msgbus,
        cache,
        clock,
        logger,
    )


@pytest.fixture(scope="function")
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


@pytest.fixture(scope="function")
def strategy_id(strategy):
    return strategy.id


@pytest.fixture(scope="function")
def components(data_engine, exec_engine, risk_engine, strategy):
    return


# TO BE IMPLEMENTED IN ADAPTER conftest.py


@pytest.fixture(scope="function")
def data_client(data_engine):
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")


@pytest.fixture(scope="function")
def exec_client(exec_engine):
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")


@pytest.fixture(scope="function")
def instrument(exec_engine):
    raise NotImplementedError("Needs to be implemented in adapter `conftest.py`")
