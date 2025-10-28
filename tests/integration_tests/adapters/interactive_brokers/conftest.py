# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from tests.integration_tests.adapters.interactive_brokers.mock_client import MockInteractiveBrokersClient
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


def mocked_ib_client(
    loop,
    msgbus,
    cache,
    clock,
    host,
    port,
    client_id,
    **kwargs,
) -> MockInteractiveBrokersClient:
    client = MockInteractiveBrokersClient(
        loop=loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        host=host,
        port=port,
        client_id=client_id,
    )
    return client


@pytest.fixture()
def venue():
    return IB_VENUE


@pytest.fixture()
def instrument():
    return IBTestContractStubs.aapl_instrument()


@pytest.fixture()
def gateway_config():
    return DockerizedIBGatewayConfig(
        username="test",
        password="test",
    )


@pytest.fixture()
def data_client_config():
    return InteractiveBrokersDataClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=0,
        ibg_client_id=1,
    )


@pytest.fixture()
def exec_client_config():
    return InteractiveBrokersExecClientConfig(
        ibg_host="127.0.0.1",
        ibg_port=0,
        ibg_client_id=1,
        account_id="DU123456",
    )


@pytest.fixture()
def ib_client(data_client_config, event_loop, msgbus, cache, clock):
    client = InteractiveBrokersClient(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        host=data_client_config.ibg_host,
        port=data_client_config.ibg_port,
        client_id=data_client_config.ibg_client_id,
    )
    yield client
    if client.is_running:
        client._stop()


@pytest.fixture()
def ib_client_running(ib_client):
    ib_client._connect = AsyncMock()
    ib_client._eclient = MagicMock()
    ib_client._eclient.startApi = MagicMock(side_effect=ib_client._is_ib_connected.set)
    ib_client._account_ids = {"DU123456,"}
    ib_client.start()
    yield ib_client

    # Cleanup: stop the client and cancel its background tasks
    if not ib_client.is_stopped:
        ib_client.stop()


@pytest.fixture()
def instrument_provider(ib_client):
    from nautilus_trader.common.component import LiveClock

    return InteractiveBrokersInstrumentProvider(
        client=ib_client,
        clock=LiveClock(),
        config=InteractiveBrokersInstrumentProviderConfig(),
    )


@pytest.fixture()
@patch(
    "nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client",
    new=mocked_ib_client,
)
@patch(
    "nautilus_trader.adapters.interactive_brokers.factories.get_cached_interactive_brokers_instrument_provider",
    new=InteractiveBrokersInstrumentProvider,
)
def data_client(data_client_config, venue, event_loop, msgbus, cache, clock):
    client = InteractiveBrokersLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=data_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    client._client._is_ib_connected.set()
    client._client._connect = AsyncMock()
    client._client._account_ids = {"DU123456,"}
    return client


@pytest.fixture()
@patch(
    "nautilus_trader.adapters.interactive_brokers.factories.get_cached_ib_client",
    new=mocked_ib_client,
)
@patch(
    "nautilus_trader.adapters.interactive_brokers.factories.get_cached_interactive_brokers_instrument_provider",
    new=InteractiveBrokersInstrumentProvider,
)
def exec_client(exec_client_config, venue, event_loop, msgbus, cache, clock):
    client = InteractiveBrokersLiveExecClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=exec_client_config,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    client._client._is_ib_connected.set()
    client._client._connect = AsyncMock()
    client._client._account_ids = {"DU123456,"}
    return client


@pytest.fixture()
def account_state(venue: Venue) -> AccountState:
    return TestEventStubs.cash_account_state(account_id=AccountId(f"{venue.value}-001"))
