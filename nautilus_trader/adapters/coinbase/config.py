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
"""Configuration for Coinbase adapter."""

from typing import Annotated

from pydantic import Field
from pydantic import PositiveInt

from nautilus_trader.adapters.coinbase.constants import COINBASE_VENUE
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.model.identifiers import Venue


class CoinbaseDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``CoinbaseDataClient`` instances.

    Parameters
    ----------
    venue : Venue, default COINBASE
        The venue for the client.
    api_key : str, optional
        The Coinbase API key.
        If ``None`` then will source from the `COINBASE_API_KEY` environment variable.
    api_secret : str, optional
        The Coinbase API secret.
        If ``None`` then will source from the `COINBASE_API_SECRET` environment variable.
    base_url_http : str, optional
        The HTTP base URL for the Coinbase API.
        If ``None`` then will use the default production URL.
    base_url_ws : str, optional
        The WebSocket base URL for the Coinbase API.
        If ``None`` then will use the default production URL.
    http_timeout_secs : PositiveInt, default 60
        The HTTP request timeout in seconds.
    update_instruments_interval_mins : PositiveInt, optional
        The interval (minutes) between instrument updates.
        If ``None`` then will not update instruments automatically.

    """

    venue: Venue = COINBASE_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt = 60
    update_instruments_interval_mins: PositiveInt | None = None


class CoinbaseExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``CoinbaseExecutionClient`` instances.

    Parameters
    ----------
    venue : Venue, default COINBASE
        The venue for the client.
    api_key : str, optional
        The Coinbase API key.
        If ``None`` then will source from the `COINBASE_API_KEY` environment variable.
    api_secret : str, optional
        The Coinbase API secret.
        If ``None`` then will source from the `COINBASE_API_SECRET` environment variable.
    base_url_http : str, optional
        The HTTP base URL for the Coinbase API.
        If ``None`` then will use the default production URL.
    base_url_ws : str, optional
        The WebSocket base URL for the Coinbase API.
        If ``None`` then will use the default production URL.
    http_timeout_secs : PositiveInt, default 60
        The HTTP request timeout in seconds.
    update_instruments_interval_mins : PositiveInt, optional
        The interval (minutes) between instrument updates.
        If ``None`` then will not update instruments automatically.

    """

    venue: Venue = COINBASE_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt = 60
    update_instruments_interval_mins: PositiveInt | None = None

