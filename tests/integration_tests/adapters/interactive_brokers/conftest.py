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
from ib_insync import IB

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers.factories import (
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


@pytest.fixture()
def venue():
    return IB_VENUE


@pytest.fixture()
def instrument():
    return IBTestProviderStubs.aapl_instrument()


@pytest.fixture()
def client() -> IB:
    return IB()


@pytest.fixture()
def data_client_config():
    return InteractiveBrokersDataClientConfig(
        username="test",
        password="test",
        account_id="DU123456",
    )


@pytest.fixture()
def exec_client_config():
    return InteractiveBrokersExecClientConfig(
        username="test",
        password="test",
        account_id="DU123456",
    )


@pytest.fixture()
def instrument_provider(client, logger):
    return InteractiveBrokersInstrumentProvider(
        client=client,
        config=InstrumentProviderConfig(),
        logger=logger,
    )


@pytest.fixture()
def data_client(mocker, data_client_config, venue, event_loop, msgbus, cache, clock, logger):
    mocker.patch("nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client")
    return InteractiveBrokersLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=data_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
    )


@pytest.fixture()
def exec_client(mocker, exec_client_config, venue, event_loop, msgbus, cache, clock, logger):
    mocker.patch(
        "nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client",
        return_value=IB(),
    )
    return InteractiveBrokersLiveExecClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=exec_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
    )


@pytest.fixture()
def account_state(venue) -> AccountState:
    return TestEventStubs.cash_account_state(account_id=AccountId(f"{venue.value}-001"))
