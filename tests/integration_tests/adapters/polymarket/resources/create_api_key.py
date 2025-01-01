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

import os

from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON


def create_polymarket_api_key():
    host = "https://clob.polymarket.com"
    key = os.environ["POLYMARKET_PK"]
    chain_id = POLYGON
    client = ClobClient(host, key=key, chain_id=chain_id)

    print(client.create_api_key())
