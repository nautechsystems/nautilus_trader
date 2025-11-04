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
Configuration for Gate.io adapter.
"""

from nautilus_trader.adapters.env import get_env_key
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class GateioDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for `GateioDataClient` instances.

    Parameters
    ----------
    api_key : str, optional
        The Gate.io API key.
        If ``None`` then will source from the `GATEIO_API_KEY` environment variable.
    api_secret : str, optional
        The Gate.io API secret.
        If ``None`` then will source from the `GATEIO_API_SECRET` environment variable.
    base_url_http : str, optional
        The base HTTP URL for the Gate.io API.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_spot : str, optional
        The base WebSocket URL for Gate.io spot markets.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_futures : str, optional
        The base WebSocket URL for Gate.io futures markets.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_options : str, optional
        The base WebSocket URL for Gate.io options markets.
        If ``None`` then will use the default mainnet URL.
    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws_spot: str | None = None
    base_url_ws_futures: str | None = None
    base_url_ws_options: str | None = None

    def __post_init__(self) -> None:
        """
        Post-initialization to set credentials from environment if not provided.
        """
        if self.api_key is None:
            object.__setattr__(self, "api_key", get_env_key("GATEIO_API_KEY"))
        if self.api_secret is None:
            object.__setattr__(self, "api_secret", get_env_key("GATEIO_API_SECRET"))


class GateioExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for `GateioExecutionClient` instances.

    Parameters
    ----------
    api_key : str, optional
        The Gate.io API key.
        If ``None`` then will source from the `GATEIO_API_KEY` environment variable.
    api_secret : str, optional
        The Gate.io API secret.
        If ``None`` then will source from the `GATEIO_API_SECRET` environment variable.
    base_url_http : str, optional
        The base HTTP URL for the Gate.io API.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_spot : str, optional
        The base WebSocket URL for Gate.io spot markets.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_futures : str, optional
        The base WebSocket URL for Gate.io futures markets.
        If ``None`` then will use the default mainnet URL.
    base_url_ws_options : str, optional
        The base WebSocket URL for Gate.io options markets.
        If ``None`` then will use the default mainnet URL.
    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws_spot: str | None = None
    base_url_ws_futures: str | None = None
    base_url_ws_options: str | None = None

    def __post_init__(self) -> None:
        """
        Post-initialization to set credentials from environment if not provided.
        """
        if self.api_key is None:
            object.__setattr__(self, "api_key", get_env_key("GATEIO_API_KEY"))
        if self.api_secret is None:
            object.__setattr__(self, "api_secret", get_env_key("GATEIO_API_SECRET"))
