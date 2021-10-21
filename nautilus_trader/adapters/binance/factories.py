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

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


HTTP_CLIENTS = {}


def get_binance_http_client(
    key: str,
    secret: str,
    loop: asyncio.AbstractEventLoop,
    clock: LiveClock,
    logger: Logger,
) -> BinanceHttpClient:
    global HTTP_CLIENTS
    client_key = (key, secret)
    if client_key not in HTTP_CLIENTS:
        print("Creating new instance of BinanceHttpClient")  # TODO(cs): debugging
        client = BinanceHttpClient(
            loop=loop,
            clock=clock,
            logger=logger,
            key=key,
            secret=secret,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]
