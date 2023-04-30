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
import json
import os

import pytest

from nautilus_trader.adapters.deribit.http.client import DeribitHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.mark.asyncio
async def test_deribit_http_client() -> None:
    loop = asyncio.get_event_loop()
    clock = LiveClock()

    client: DeribitHttpClient = DeribitHttpClient(
        loop=loop,
        clock=clock,
        logger=Logger(clock=clock),
        key=os.getenv("DERIBIT_API_KEY"),
        secret=os.getenv("DERIBIT_API_SECRET"),
    )

    await client.connect()

    # Test authentication works with account info
    response = await client.access_log()

    print(json.dumps(response, indent=4))

    await client.disconnect()
