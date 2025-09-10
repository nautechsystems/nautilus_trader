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
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode


class OKXDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``OKXDataClient`` instances.

    Parameters
    ----------
    api_key : str, [default=None]
        The OKX API public key.
        If ``None`` then will source the `OKX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The OKX API secret key.
        If ``None`` then will source the `OKX_API_SECRET` environment variable.
    api_passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], default `(OKXInstrumentType.SPOT,)`
        The OKX instrument types of instruments to load.
        If None, all instrument types are loaded (subject to contract types and their compatibility with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load.
        If None, all contract types are loaded (subject to instrument types and their compatibility with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `get_http_base_url()`.
    base_url_ws : str, optional
        The base url to OKX's websocket API.
        If ``None`` then will source the url from `get_ws_base_url()`.
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    vip_level : int, optional
        The account VIP level to determine book subscriptions.
        - Only VIP4 and above in trading fee tier are allowed to subscribe to "books50-l2-tbt" 50 depth channels (10 ms updates)
        - Only VIP5 and above in trading fee tier are allowed to subscribe to "books-l2-tbt" 400 depth channels (10 ms updates)

    """

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType, ...] = (OKXInstrumentType.SPOT,)
    contract_types: tuple[OKXContractType, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_demo: bool = False
    http_timeout_secs: PositiveInt | None = 60
    update_instruments_interval_mins: PositiveInt | None = 60
    vip_level: PositiveInt | None = None  # TODO: OKXVipLevel enum


class OKXExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``OKXExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, [default=None]
        The OKX API public key.
        If ``None`` then will source the `OKX_API_KEY` environment variable.
    api_secret : str, [default=None]
        The OKX API secret key.
        If ``None`` then will source the `OKX_API_SECRET` environment variable.
    api_passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], default `(OKXInstrumentType.SPOT,)`
        The OKX instrument types of instruments to load.
        If None, all instrument types are loaded (subject to contract types and their compatibility with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load.
        If None, all contract types are loaded (subject to instrument types and their compatibility with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `get_http_base_url()`.
    base_url_ws : str, optional
        The base url to OKX's websocket API.
        If ``None`` then will source the url from `get_ws_base_url()`.
    margin_mode : OKXMarginMode, optional
        The intended OKX account margin mode (referred to as mgnMode by OKX's docs).
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.
    use_fills_channel : bool, default False
        If True, subscribes to the fills channel for separate fill reports (requires VIP5+).
        If False, generates fill reports from order status reports (works for all users).
    use_mm_mass_cancel : bool, default False
        If True, uses OKX's mass-cancel endpoint for cancel_all_orders operations.
        This endpoint is typically restricted to market makers and high-volume traders.
        If False, cancels orders individually (works for all users).
    max_retries : PositiveInt, default 3
        The maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) for retries.
    retry_delay_max_ms : PositiveInt, default 10_000
        The maximum delay (milliseconds) for exponential backoff.

    """

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType, ...] = (OKXInstrumentType.SPOT,)
    contract_types: tuple[OKXContractType, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    margin_mode: OKXMarginMode | None = None
    is_demo: bool = False
    http_timeout_secs: PositiveInt | None = 60
    use_fills_channel: bool = False
    use_mm_mass_cancel: bool = False
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
