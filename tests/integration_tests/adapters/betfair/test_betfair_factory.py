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

import asyncio

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factory import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factory import BetfairLiveExecutionClientFactory


@pytest.mark.asyncio()
def test_create(mocker, msgbus, cache, clock, live_logger):
    config = {
        "data_client": True,
        "exec_client": True,
        "base_currency": "AUD",
    }

    # TODO - Fix mock for login assertion
    # Mock client
    mocker.patch("betfairlightweight.endpoints.login.Login.__call__")
    # mock_login = mocker.patch("betfairlightweight.endpoints.login.Login.request")

    data_client = BetfairLiveDataClientFactory.create(
        loop=asyncio.get_event_loop(),
        name=BETFAIR_VENUE.value,
        config=config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=live_logger,
    )
    exec_client = BetfairLiveExecutionClientFactory.create(
        loop=asyncio.get_event_loop(),
        name=BETFAIR_VENUE.value,
        config=config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=live_logger,
    )

    # Assert
    assert BetfairDataClient == type(data_client)
    assert BetfairExecutionClient == type(exec_client)
    # TODO - assert login called
    # assert mock_login.assert_called_once_with()
