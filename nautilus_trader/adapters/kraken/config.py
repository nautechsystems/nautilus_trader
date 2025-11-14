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

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import PositiveInt


class KrakenDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``KrakenDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API public key.
        If ``None`` then will source the `KRAKEN_API_KEY`
        environment variable.
    api_secret : str, optional
        The Kraken API secret key.
        If ``None`` then will source the `KRAKEN_API_SECRET`
        environment variable.
    product_types : tuple[str, ...], optional
        The Kraken product types for the client.
        Options: ("spot", "futures"). If ``None`` then defaults to ("spot",).
    base_url_http : str, optional
        The base URL for Kraken HTTP API.
        If ``None`` then will use the default production URL.
    base_url_ws : str, optional
        The base URL for Kraken WebSocket API.
        If ``None`` then will use the default production URL.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    ws_proxy_url : str, optional
        Optional WebSocket proxy URL.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    max_retries : PositiveInt, optional
        The maximum number of times an HTTP request will be retried.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.
    http_timeout_secs : PositiveInt, optional
        The timeout in seconds for HTTP requests.
    ws_heartbeat_secs : PositiveInt, default 30
        The WebSocket heartbeat interval in seconds.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: tuple[str, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    ws_proxy_url: str | None = None
    update_instruments_interval_mins: PositiveInt | None = 60
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None
    http_timeout_secs: PositiveInt | None = None
    ws_heartbeat_secs: PositiveInt = 30
