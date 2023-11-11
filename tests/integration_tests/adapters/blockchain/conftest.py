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
import os

import pytest

from nautilus_trader.adapters.blockchain.factories import get_cached_blockchain_http_client
from nautilus_trader.adapters.blockchain.http.client import BlockchainHttpClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


@pytest.fixture()
def instrument():
    pass


@pytest.fixture()
def data_client():
    pass


@pytest.fixture()
def exec_client():
    pass


@pytest.fixture()
def account_state():
    pass


@pytest.fixture()
def client() -> BlockchainHttpClient:
    clock = LiveClock()
    rpc_url = os.getenv("RPC_URL", "http://localhost:8545")
    return get_cached_blockchain_http_client(
        clock=clock,
        logger=Logger(clock=clock),
        rpc_url=rpc_url,
    )
