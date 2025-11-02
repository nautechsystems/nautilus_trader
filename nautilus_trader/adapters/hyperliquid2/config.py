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

from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class HyperliquidDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``HyperliquidDataClient`` instances.

    Parameters
    ----------
    private_key : str, default None
        The Hyperliquid private key for wallet-based authentication.
        If ``None`` then will source the `HYPERLIQUID_PRIVATE_KEY` environment variable.
    wallet_address : str, default None
        The Hyperliquid wallet address.
        If ``None`` then will source the `HYPERLIQUID_WALLET_ADDRESS` environment variable.
    base_url_http : str, default None
        The base URL for Hyperliquid HTTP API.
        If ``None`` then will use the default mainnet URL.
    base_url_ws : str, default None
        The base URL for Hyperliquid WebSocket API.
        If ``None`` then will use the default mainnet URL.
    testnet : bool, default False
        If the client should connect to testnet instead of mainnet.
    http_timeout_secs : PositiveInt, default 60
        The HTTP timeout in seconds for requests.
    update_instruments_interval_mins : PositiveInt, default 60
        The interval (minutes) between reloading instruments from the venue.

    """

    private_key: str | None = None
    wallet_address: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    update_instruments_interval_mins: PositiveInt | None = 60


class HyperliquidExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``HyperliquidExecutionClient`` instances.

    Parameters
    ----------
    private_key : str, default None
        The Hyperliquid private key for wallet-based authentication.
        If ``None`` then will source the `HYPERLIQUID_PRIVATE_KEY` environment variable.
    wallet_address : str, default None
        The Hyperliquid wallet address.
        If ``None`` then will source the `HYPERLIQUID_WALLET_ADDRESS` environment variable.
    base_url_http : str, default None
        The base URL for Hyperliquid HTTP API.
        If ``None`` then will use the default mainnet URL.
    base_url_ws : str, default None
        The base URL for Hyperliquid WebSocket API.
        If ``None`` then will use the default mainnet URL.
    testnet : bool, default False
        If the client should connect to testnet instead of mainnet.
    http_timeout_secs : PositiveInt, default 60
        The HTTP timeout in seconds for requests.

    """

    private_key: str | None = None
    wallet_address: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
