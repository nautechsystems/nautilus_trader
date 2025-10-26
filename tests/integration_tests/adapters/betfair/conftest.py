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

import asyncio
from typing import Any
from unittest.mock import patch

import pytest
import pytest_asyncio

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
from tests.integration_tests.adapters.betfair.test_kit import load_betfair_data


@pytest.fixture()
def instrument():
    return betting_instrument()


@pytest.fixture()
def venue() -> Venue:
    return BETFAIR_VENUE


@pytest.fixture()
def account_state() -> AccountState:
    return TestEventStubs.betting_account_state(account_id=AccountId("BETFAIR-001"))


@pytest.fixture()
def betfair_client(event_loop):
    return BetfairTestStubs.betfair_client(event_loop)


@pytest.fixture()
def instrument_provider(betfair_client):
    config = BetfairInstrumentProviderConfig(
        account_currency="GBP",
        event_type_ids=[1, 2],
    )
    return BetfairTestStubs.instrument_provider(
        betfair_client=betfair_client,
        config=config,
    )


@pytest.fixture()
def data_client(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
) -> BetfairDataClient:
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )

    instrument_provider.add(instrument)
    data_client = BetfairLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairDataClientConfig(
            account_currency="GBP",
            username="username",
            password="password",
            app_key="app_key",
        ),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    data_client._client._headers.update(
        {
            "X-Authentication": "token",
            "X-Application": "product",
        },
    )

    # Patches
    patch(
        "nautilus_trader.adapters.betfair.data.BetfairDataClient._instrument_provider.get_account_currency",
        return_value="GBP",
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.data.BetfairDataClient.stream_subscribe",
    )
    patch(
        "nautilus_trader.adapters.betfair.providers.BetfairInstrumentProvider._client.list_navigation",
        return_value=BetfairResponses.navigation_list_navigation(),
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.data.BetfairDataClient.stream_subscribe",
    )

    return data_client


@pytest.fixture()
def exec_client(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
) -> BetfairExecutionClient:
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )

    instrument_provider.add(instrument)
    exec_client: BetfairExecutionClient = BetfairLiveExecClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairExecClientConfig(
            username="username",
            password="password",
            app_key="app_key",
            account_currency="GBP",
        ),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    exec_client._client._headers.update(
        {
            "X-Authentication": "token",
            "X-Application": "product",
        },
    )

    return exec_client


@pytest.fixture()
def data_catalog(tmp_path) -> ParquetDataCatalog:
    catalog: ParquetDataCatalog = setup_catalog(protocol="memory", path=tmp_path / "catalog")
    load_betfair_data(catalog)
    return catalog


@pytest.fixture()
def parser() -> BetfairParser:
    return BetfairParser(currency="GBP")


async def handle_echo(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    writer.write(b"connected\r\n")

    while True:
        data = await reader.read(1024)
        if not data or data == b"close":
            break
        writer.write(data)
        await writer.drain()
    writer.close()
    await writer.wait_closed()


@pytest_asyncio.fixture()
async def socket_server():
    try:
        server = await asyncio.start_server(handle_echo, "127.0.0.1", 0)
    except (PermissionError, OSError) as e:
        if isinstance(e, PermissionError) or getattr(e, "errno", None) in {1, 13}:
            pytest.skip("Unable to create local socket server in restricted environment.")
            raise
        raise
    if not server.sockets:
        pytest.skip("Unable to create local socket server in restricted environment.")
    addr = server.sockets[0].getsockname()
    await server.start_serving()

    try:
        yield addr
    finally:
        server.close()
        await server.wait_closed()


@pytest_asyncio.fixture(name="closing_socket_server")
async def fixture_closing_socket_server():
    async def handler(_: Any, writer: asyncio.StreamWriter) -> None:
        async def write():
            print("SERVER CONNECTING")
            writer.write(b"connected\r\n")
            await asyncio.sleep(0.5)
            await writer.drain()
            writer.close()
            await writer.wait_closed()
            writer._transport.abort()
            await asyncio.sleep(0.1)
            print("Server closed")

        await write()

    try:
        server = await asyncio.start_server(handler, "127.0.0.1", 0)
    except (PermissionError, OSError) as e:
        if isinstance(e, PermissionError) or getattr(e, "errno", None) in {1, 13}:
            pytest.skip("Unable to create local socket server in restricted environment.")
            raise
        raise
    if not server.sockets:
        pytest.skip("Unable to create local socket server in restricted environment.")
    addr = server.sockets[0].getsockname()

    try:
        yield addr
    finally:
        server.close()
        await server.wait_closed()
