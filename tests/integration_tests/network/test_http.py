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

import asyncio

import pytest
import pytest_asyncio

from nautilus_trader.network.http import HttpClient
from nautilus_trader.test_kit.stubs.component import TestComponentStubs


@pytest_asyncio.fixture()
async def client():
    client = HttpClient(
        loop=asyncio.get_event_loop(),
        logger=TestComponentStubs.logger(),
    )
    await client.connect()
    return client


@pytest.mark.skip(reason="Relies on external internet resource")
@pytest.mark.asyncio
async def test_client_get(client):
    resp = await client.get("https://httpbin.org/get")
    assert len(resp.data) > 100
    assert client.max_latency() > 0
    assert client.min_latency() > 0
    assert client.avg_latency() > 0


@pytest.mark.skip(reason="Relies on external internet resource")
@pytest.mark.asyncio
async def test_client_post(client):
    resp = await client.post("https://httpbin.org/post")
    assert len(resp.data) > 100
