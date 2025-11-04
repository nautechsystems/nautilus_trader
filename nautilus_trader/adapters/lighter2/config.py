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


class LighterDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``LighterDataClient`` instances.

    Parameters
    ----------
    api_key_private_key : str, optional
        The Lighter API private key.
        If ``None`` then will source the `LIGHTER_API_KEY_PRIVATE_KEY` environment variable.
    eth_private_key : str, optional
        The Ethereum private key for transaction signing.
        If ``None`` then will source the `LIGHTER_ETH_PRIVATE_KEY` environment variable.
    base_url_http : str, optional
        The base URL for Lighter's HTTP API.
        If ``None`` then will use the mainnet URL.
    base_url_ws : str, optional
        The base URL for Lighter's WebSocket API.
        If ``None`` then will use the mainnet WebSocket URL.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    ws_proxy_url : str, optional
        Optional WebSocket proxy URL.
    is_testnet : bool, default False
        If the client is connecting to the Lighter testnet.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    account_type : str, default "standard"
        The account type: "standard" (fee-less) or "premium" (0.2 bps maker, 2 bps taker).

    """

    api_key_private_key: str | None = None
    eth_private_key: str | None = None
    api_key_index: int | None = None
    account_index: int | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    ws_proxy_url: str | None = None
    is_testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    update_instruments_interval_mins: PositiveInt | None = 60
    account_type: str = "standard"


class LighterExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``LighterExecutionClient`` instances.

    Parameters
    ----------
    api_key_private_key : str, optional
        The Lighter API private key.
        If ``None`` then will source the `LIGHTER_API_KEY_PRIVATE_KEY` environment variable.
    eth_private_key : str, optional
        The Ethereum private key for transaction signing.
        If ``None`` then will source the `LIGHTER_ETH_PRIVATE_KEY` environment variable.
    base_url_http : str, optional
        The base URL for Lighter's HTTP API.
        If ``None`` then will use the mainnet URL.
    base_url_ws : str, optional
        The base URL for Lighter's WebSocket API.
        If ``None`` then will use the mainnet WebSocket URL.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    ws_proxy_url : str, optional
        Optional WebSocket proxy URL.
    is_testnet : bool, default False
        If the client is connecting to the Lighter testnet.
    account_type : str, default "standard"
        The account type: "standard" (fee-less) or "premium" (0.2 bps maker, 2 bps taker).
    max_retries : PositiveInt, default 3
        The maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, default 10_000
        The maximum delay (milliseconds) between retries.
    use_nonce_management : bool, default True
        If True, automatically manages nonce increments for transactions.
        If False, user must handle nonce management manually.

    """

    api_key_private_key: str | None = None
    eth_private_key: str | None = None
    api_key_index: int | None = None
    account_index: int | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    ws_proxy_url: str | None = None
    is_testnet: bool = False
    account_type: str = "standard"
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    use_nonce_management: bool = True