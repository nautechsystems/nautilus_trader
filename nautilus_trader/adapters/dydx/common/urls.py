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
"""
Define base urls for HTTP endpoints and websocket data streams.
"""


def get_http_base_url(is_testnet: bool) -> str:
    """
    Provide the base HTTP url for dYdX.
    """
    if is_testnet:
        return "https://indexer.v4testnet.dydx.exchange/v4"

    return "https://indexer.dydx.trade/v4"


def get_ws_base_url(is_testnet: bool) -> str:
    """
    Provide the base websockets url for dYdX.
    """
    if is_testnet:
        return "wss://indexer.v4testnet.dydx.exchange/v4/ws"

    return "wss://indexer.dydx.trade/v4/ws"


def get_grpc_base_url(is_testnet: bool) -> str:
    """
    Provide the base GRPC url for dYdX.
    """
    if is_testnet:
        return "test-dydx-grpc.kingnodes.com"

    return "dydx-ops-grpc.kingnodes.com:443"
