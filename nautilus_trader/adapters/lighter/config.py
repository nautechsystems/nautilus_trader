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

from __future__ import annotations

from nautilus_trader.adapters.lighter.credentials import resolve_account_index
from nautilus_trader.adapters.lighter.credentials import resolve_api_key_private_key
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class LighterDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``LighterDataClient`` instances.

    Parameters
    ----------
    api_key_private_key : str, optional
        The API key private key used to create auth tokens or signatures.
        If ``None`` then will source the ``LIGHTER_API_KEY_PRIVATE_KEY`` or
        ``LIGHTER_TESTNET_API_KEY_PRIVATE_KEY`` environment variable depending on
        the `testnet` setting.
    account_index : int, optional
        The Lighter account index for the key.
        If ``None`` then will source ``LIGHTER_ACCOUNT_INDEX`` or
        ``LIGHTER_TESTNET_ACCOUNT_INDEX`` depending on `testnet`.
    api_key_index : int, default 2
        The API key slot index (default aligns with common UI convention).
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    ws_proxy_url : str, optional
        Optional WebSocket proxy URL.
        Note: WebSocket proxy support is not yet implemented. This field is reserved
        for future functionality. Use `http_proxy_url` for REST API proxy support.
    testnet : bool, default False
        If the client is connecting to the Lighter testnet API.
    update_instrument_interval_ms : PositiveInt, default 3_600_000
        Interval (milliseconds) between refreshing instrument metadata.
    http_timeout_secs : PositiveInt, default 10
        Timeout (seconds) for HTTP requests.
    """

    api_key_private_key: str | None = None
    account_index: int | None = None
    api_key_index: int = 2
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    ws_proxy_url: str | None = None
    testnet: bool = False
    update_instrument_interval_ms: PositiveInt = 3_600_000
    http_timeout_secs: PositiveInt = 10

    @property
    def resolved_api_key_private_key(self) -> str | None:
        return resolve_api_key_private_key(self.api_key_private_key, testnet=self.testnet)

    @property
    def resolved_account_index(self) -> int | None:
        return resolve_account_index(self.account_index, testnet=self.testnet)


class LighterExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``LighterExecutionClient`` instances.

    Parameters
    ----------
    api_key_private_key : str, optional
        The API key private key used to create auth tokens or signatures.
        If ``None`` then will source the ``LIGHTER_API_KEY_PRIVATE_KEY`` or
        ``LIGHTER_TESTNET_API_KEY_PRIVATE_KEY`` environment variable depending on
        the `testnet` setting.
    account_index : int, optional
        The Lighter account index for the key.
        If ``None`` then will source ``LIGHTER_ACCOUNT_INDEX`` or
        ``LIGHTER_TESTNET_ACCOUNT_INDEX`` depending on `testnet`.
    api_key_index : int, default 2
        The API key slot index (default aligns with common UI convention).
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    ws_proxy_url : str, optional
        Optional WebSocket proxy URL.
    testnet : bool, default False
        If the client is connecting to the Lighter testnet API.
    max_retries : PositiveInt, default 3
        Maximum retry attempts for order requests.
    retry_delay_ms : PositiveInt, default 1_000
        Delay (milliseconds) between retries.
    http_timeout_secs : PositiveInt, default 10
        Timeout (seconds) for HTTP requests.
    """

    api_key_private_key: str | None = None
    account_index: int | None = None
    api_key_index: int = 2
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    ws_proxy_url: str | None = None
    testnet: bool = False
    max_retries: PositiveInt = 3
    retry_delay_ms: PositiveInt = 1_000
    http_timeout_secs: PositiveInt = 10

    @property
    def resolved_api_key_private_key(self) -> str | None:
        return resolve_api_key_private_key(self.api_key_private_key, testnet=self.testnet)

    @property
    def resolved_account_index(self) -> int | None:
        return resolve_account_index(self.account_index, testnet=self.testnet)
