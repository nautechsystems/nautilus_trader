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


class BitmexDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BitmexDataClient`` instances.

    Parameters
    ----------
    api_key : str, [default=None]
        The BitMEX API public key.
        If ``None`` then will source the `BITMEX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The BitMEX API secret key.
        If ``None`` then will source the `BITMEX_API_SECRET` environment variable.
    base_url_http : str, optional
        The base url to BitMEX's HTTP API.
        If ``None`` then will use the default production URL.
    base_url_ws : str, optional
        The base url to BitMEX's WebSocket API.
        If ``None`` then will use the default production URL.
    testnet : bool, default False
        If the client is connecting to the BitMEX testnet.
    http_timeout_secs : PositiveInt, default 60
        The timeout for HTTP requests in seconds.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.

    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    update_instruments_interval_mins: PositiveInt | None = 60


class BitmexExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BitmexExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, [default=None]
        The BitMEX API public key.
        If ``None`` then will source the `BITMEX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The BitMEX API secret key.
        If ``None`` then will source the `BITMEX_API_SECRET` environment variable.
    base_url_http : str, optional
        The base url to BitMEX's HTTP API.
        If ``None`` then will use the default production URL.
    base_url_ws : str, optional
        The base url to BitMEX's WebSocket API.
        If ``None`` then will use the default production URL.
    testnet : bool, default False
        If the client is connecting to the BitMEX testnet.
    http_timeout_secs : PositiveInt, default 60
        The timeout for HTTP requests in seconds.
    max_retries : PositiveInt, optional
        The maximum number of retries for HTTP requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) for retries.
    retry_delay_max_ms : PositiveInt, default 5_000
        The maximum delay (milliseconds) for exponential backoff.

    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 5_000
