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
from functools import lru_cache

from nautilus_trader.adapters.blockchain.http.client import BlockchainHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


HTTP_CLIENTS: dict[str, BlockchainHttpClient] = {}


@lru_cache(1)
def get_cached_blockchain_http_client(
    clock: LiveClock,
    logger: Logger,
    rpc_url: str | None = None,
):
    global HTTP_CLIENTS
    rpc = rpc_url or "http://localhost:8545"
    if rpc not in HTTP_CLIENTS:
        client = BlockchainHttpClient(clock, logger, rpc)
        HTTP_CLIENTS[rpc] = client
    return HTTP_CLIENTS[rpc]
