# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.nautilus_pyo3 import AxEnvironment


class AxDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``AxDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The AX Exchange API key.
        If ``None`` then will source the `AX_API_KEY` environment variable.
    api_secret : str, optional
        The AX Exchange API secret.
        If ``None`` then will source the `AX_API_SECRET` environment variable.
    environment : AxEnvironment, default AxEnvironment.SANDBOX
        The AX Exchange environment to connect to (Sandbox or Production).
    base_url_http : str, optional
        The base URL for the AX Exchange HTTP API.
        If ``None`` then will use the URL for the configured environment.
    base_url_ws : str, optional
        The base URL for the AX Exchange WebSocket API.
        If ``None`` then will use the URL for the configured environment.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    http_timeout_secs : PositiveInt, optional
        HTTP request timeout in seconds.
    max_retries : PositiveInt, optional
        Maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, optional
        Initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        Maximum delay (milliseconds) between retries.
    update_instruments_interval_mins : PositiveInt, optional
        The interval (minutes) between reloading instruments from the venue.
    funding_rate_poll_interval_mins : PositiveInt, optional
        The interval (minutes) between polling for funding rate updates.

    """

    api_key: str | None = None
    api_secret: str | None = None
    environment: AxEnvironment = AxEnvironment.SANDBOX
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    update_instruments_interval_mins: PositiveInt | None = 60
    funding_rate_poll_interval_mins: PositiveInt | None = 15


class AxExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``AxExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The AX Exchange API key.
        If ``None`` then will source the `AX_API_KEY` environment variable.
    api_secret : str, optional
        The AX Exchange API secret.
        If ``None`` then will source the `AX_API_SECRET` environment variable.
    environment : AxEnvironment, default AxEnvironment.SANDBOX
        The AX Exchange environment to connect to (Sandbox or Production).
    base_url_http : str, optional
        The base URL for the AX Exchange HTTP API.
        If ``None`` then will use the URL for the configured environment.
    base_url_ws : str, optional
        The base URL for the AX Exchange WebSocket API.
        If ``None`` then will use the URL for the configured environment.
    http_proxy_url : str, optional
        Optional HTTP proxy URL.
    http_timeout_secs : PositiveInt, optional
        HTTP request timeout in seconds.
    max_retries : PositiveInt, optional
        Maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, optional
        Initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        Maximum delay (milliseconds) between retries.

    """

    api_key: str | None = None
    api_secret: str | None = None
    environment: AxEnvironment = AxEnvironment.SANDBOX
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_proxy_url: str | None = None
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
