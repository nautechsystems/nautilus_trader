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
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType


class DeribitDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``DeribitDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Deribit API public key.
        If ``None`` then will source the `DERIBIT_API_KEY` or `DERIBIT_TESTNET_API_KEY`
        environment variable based on `is_testnet`.
    api_secret : str, optional
        The Deribit API secret key.
        If ``None`` then will source the `DERIBIT_API_SECRET` or `DERIBIT_TESTNET_API_SECRET`
        environment variable based on `is_testnet`.
    product_types : tuple[DeribitProductType, ...], optional
        The Deribit product types to load.
        If None, defaults to Future.
    base_url_http : str, optional
        The base URL for Deribit's HTTP API.
        If ``None`` then will use default based on `is_testnet`.
    base_url_ws : str, optional
        The base URL for Deribit's WebSocket API.
        If ``None`` then will use default based on `is_testnet`.
    is_testnet : bool, default False
        If the client is connecting to the Deribit testnet API.
    http_timeout_secs : PositiveInt, optional
        The timeout (seconds) for HTTP requests.
    max_retries : PositiveInt, default 3
        The maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, default 10_000
        The maximum delay (milliseconds) between retries.
    update_instruments_interval_mins : PositiveInt, default 60
        The interval (minutes) between reloading instruments from the venue.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: tuple[DeribitProductType, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    update_instruments_interval_mins: PositiveInt | None = 60


class DeribitExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``DeribitExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Deribit API public key.
        If ``None`` then will source the `DERIBIT_API_KEY` or `DERIBIT_TESTNET_API_KEY`
        environment variable based on `is_testnet`.
    api_secret : str, optional
        The Deribit API secret key.
        If ``None`` then will source the `DERIBIT_API_SECRET` or `DERIBIT_TESTNET_API_SECRET`
        environment variable based on `is_testnet`.
    product_types : tuple[DeribitProductType, ...], optional
        The Deribit product types to load.
        If None, defaults to Future.
    base_url_http : str, optional
        The base URL for Deribit's HTTP API.
        If ``None`` then will use default based on `is_testnet`.
    base_url_ws : str, optional
        The base URL for Deribit's WebSocket API.
        If ``None`` then will use default based on `is_testnet`.
    is_testnet : bool, default False
        If the client is connecting to the Deribit testnet API.
    http_timeout_secs : PositiveInt, optional
        The timeout (seconds) for HTTP requests.
    max_retries : PositiveInt, default 3
        The maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, default 10_000
        The maximum delay (milliseconds) between retries.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: tuple[DeribitProductType, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_testnet: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
