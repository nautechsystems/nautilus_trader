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

from adapters.betfair.data import BetfairDataClient
from adapters.betfair.execution import BetfairExecutionClient
from adapters.betfair.factory import BetfairClientsFactory
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


@pytest.mark.asyncio()
def test_create(mocker, data_engine, exec_engine, clock, live_logger):
    config = {
        "data_client": True,
        "exec_client": True,
    }

    # TODO - Fix mock for login assertion
    # Mock client
    mocker.patch("betfairlightweight.endpoints.login.Login.__call__")
    # mock_login = mocker.patch("betfairlightweight.endpoints.login.Login.request")

    data_client, exec_client = BetfairClientsFactory.create(
        config=config,
        data_engine=data_engine,
        exec_engine=exec_engine,
        clock=clock,
        logger=live_logger,
    )

    # Assert
    assert BetfairDataClient == type(data_client)
    assert BetfairExecutionClient == type(exec_client)
    # TODO - assert login called
    # assert mock_login.assert_called_once_with()
