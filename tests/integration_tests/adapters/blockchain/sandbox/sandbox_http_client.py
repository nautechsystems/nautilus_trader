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

from nautilus_trader.adapters.blockchain.http.client import BlockchainHttpClient
from tests.integration_tests.adapters.blockchain.utils import save_blockchain_data_to_file


force_create = True if "FORCE_CREATE" in os.environ else False
base_path = "../resources/http_responses/"


@pytest.mark.asyncio()
async def test_sandbox_get_latest_block(client: BlockchainHttpClient):
    latest_block = client.get_latest_block()
    path = base_path + "block.json"
    save_blockchain_data_to_file(path, dict(latest_block), force_create)
