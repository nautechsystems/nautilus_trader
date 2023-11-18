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

from typing import Any

import pytest
from pytest_mock import MockerFixture

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.msgbus import MessageBus
from nautilus_trader.core.message import Event
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig
from nautilus_trader.trading.trader import Trader


@pytest.fixture()
def account_id(venue):
    return AccountId(f"{venue.value}-001")


@pytest.fixture()
def clock():
    clock = TestClock()
    clock.set_time(0)
    return clock


@pytest.fixture()
def logger(clock):
    return Logger(clock, bypass=True)  # Bypass for normal testing


@pytest.fixture()
def log(logger):
    return LoggerAdapter("test", logger)


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
    if instrument is not None:
        cache.add_instrument(instrument)
    return cache


@pytest.fixture()
def portfolio(clock, logger, cache, msgbus, account_state):
    portfolio = Portfolio(
        msgbus,
        cache,
        clock,
        logger,
    )
    if account_state is not None:
        portfolio.update_account(account_state)
    return portfolio


@pytest.fixture()
def data_engine(msgbus, cache, clock, logger, data_client):
    engine = DataEngine(
        msgbus,
        cache,
        clock,
        logger,
    )
    if data_client is not None:
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
    if exec_client is not None:
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
    strategy = Strategy(config=StrategyConfig(strategy_id="S", order_id_tag="001"))
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
def trade_id(strategy):
    return TestIdStubs.trade_id()


@pytest.fixture(autouse=True)
def components(data_engine, exec_engine, risk_engine, strategy):
    # Ensures components are created and running for every test
    return


def _collect_events(msgbus, filter_types: tuple[type, ...] | None = None):
    events = []

    def handler(event: Event) -> None:
        if filter_types is None or isinstance(event, filter_types):
            events.append(event)

    msgbus.subscribe("events.*", handler=handler)
    return events


@pytest.fixture()
def events(msgbus: MessageBus) -> list[Event]:
    return _collect_events(msgbus, filter_types=None)


@pytest.fixture()
def fill_events(msgbus: MessageBus) -> list[Event]:
    return _collect_events(msgbus, filter_types=(OrderFilled,))


@pytest.fixture()
def cancel_events(msgbus: MessageBus) -> list[Event]:
    return _collect_events(msgbus, filter_types=(OrderCanceled,))


@pytest.fixture()
def messages(msgbus: MessageBus) -> list[Any]:
    messages: list[Any] = []
    msgbus.subscribe("*", handler=messages.append)
    return messages


@pytest.fixture()
def account(account_state, cache):
    return AccountFactory.create(account_state)


# TO BE IMPLEMENTED IN ADAPTER conftest.py
@pytest.fixture()
def venue() -> Venue:
    raise NotImplementedError("`venue` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def instrument_provider():
    raise NotImplementedError(
        "`instrument_provider` needs to be implemented in adapter `conftest.py`",
    )


@pytest.fixture()
def data_client():
    raise NotImplementedError("`data_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def exec_client():
    raise NotImplementedError("`exec_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def instrument():
    raise NotImplementedError("`instrument` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def account_state() -> AccountState:
    raise NotImplementedError("`account_state` needs to be implemented in adapter `conftest.py`")
