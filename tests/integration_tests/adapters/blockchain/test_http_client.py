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
import json
import pkgutil

from web3.eth import Eth

from nautilus_trader.adapters.blockchain.http.client import BlockchainHttpClient
from tests.integration_tests.adapters.blockchain.utils import get_mock


def test_get_latest_block(client: BlockchainHttpClient, monkeypatch):
    response = pkgutil.get_data(
        "tests.integration_tests.adapters.blockchain.resources.http_responses",
        "block.json",
    )
    data_json = json.loads(response)
    monkeypatch.setattr(Eth, "get_block", get_mock(data_json))
    block = client.get_latest_block()
    assert block["number"] == 18548270
