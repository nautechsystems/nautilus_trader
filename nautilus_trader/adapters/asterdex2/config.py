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
Configuration for Asterdex adapters.
"""

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


def get_env_key(key: str) -> str:
    """Return environment variable name for the given key."""
    return f"ASTERDEX_{key.upper()}"


class AsterdexDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for `AsterdexLiveDataClient` instances.

    Parameters
    ----------
    api_key : str, optional
        The Asterdex API key.
        If None, will try to read from environment variable ASTERDEX_API_KEY.
    api_secret : str, optional
        The Asterdex API secret.
        If None, will try to read from environment variable ASTERDEX_API_SECRET.
    base_url_http_spot : str, optional
        The base HTTP URL for spot API.
    base_url_http_futures : str, optional
        The base HTTP URL for futures API.
    base_url_ws_spot : str, optional
        The base WebSocket URL for spot streams.
    base_url_ws_futures : str, optional
        The base WebSocket URL for futures streams.
    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http_spot: str | None = None
    base_url_http_futures: str | None = None
    base_url_ws_spot: str | None = None
    base_url_ws_futures: str | None = None


class AsterdexExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for `AsterdexLiveExecClient` instances.

    Parameters
    ----------
    api_key : str
        The Asterdex API key.
        If None, will try to read from environment variable ASTERDEX_API_KEY.
    api_secret : str
        The Asterdex API secret.
        If None, will try to read from environment variable ASTERDEX_API_SECRET.
    base_url_http_spot : str, optional
        The base HTTP URL for spot API.
    base_url_http_futures : str, optional
        The base HTTP URL for futures API.
    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http_spot: str | None = None
    base_url_http_futures: str | None = None
