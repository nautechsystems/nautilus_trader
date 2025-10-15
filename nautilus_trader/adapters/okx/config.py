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
    instrument_families : tuple[str, ...], optional
        The OKX instrument families to load (e.g., "BTC-USD", "ETH-USD").
        Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN.
        If None, all available instrument families will be attempted (may fail for OPTIONS).
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

    """

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType, ...] = (OKXInstrumentType.SPOT,)
    instrument_families: tuple[str, ...] | None = None
    contract_types: tuple[OKXContractType, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_demo: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    update_instruments_interval_mins: PositiveInt | None = 60


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
    instrument_families : tuple[str, ...], optional
        The OKX instrument families to load (e.g., "BTC-USD", "ETH-USD").
        Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN.
        If None, all available instrument families will be attempted (may fail for OPTIONS).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `get_http_base_url()`.
    base_url_ws : str, optional
        The base url to OKX's websocket API.
        If ``None`` then will source the url from `get_ws_base_url()`.
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.
    margin_mode : OKXMarginMode, optional
        The intended OKX account margin mode for derivatives trading (SWAP/FUTURES/OPTIONS).
        - `ISOLATED`: Margin isolated to specific positions (default for derivatives)
        - `CROSS`: Margin shared across all positions
        Not applicable for SPOT trading (see `use_spot_margin` instead).
    use_spot_margin : bool, default False
        If True, enables margin/leverage for SPOT trading (uses 'spot_isolated' trade mode).
        If False, uses simple SPOT trading without leverage (uses 'cash' trade mode).
        Only applicable when trading SPOT instruments.
    max_retries : PositiveInt, default 3
        The maximum retry attempts for requests.
    retry_delay_initial_ms : PositiveInt, default 1_000
        The initial delay (milliseconds) for retries.
    retry_delay_max_ms : PositiveInt, default 10_000
        The maximum delay (milliseconds) for exponential backoff.
    use_fills_channel : bool, default False
        If True, subscribes to the fills channel for separate fill reports (requires VIP5+).
        If False, generates fill reports from order status reports (works for all users).
    use_mm_mass_cancel : bool, default False
        If True, uses OKX's mass-cancel endpoint for cancel_all_orders operations.
        This endpoint is typically restricted to market makers and high-volume traders.
        If False, cancels orders individually (works for all users).

    """

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType, ...] = (OKXInstrumentType.SPOT,)
    contract_types: tuple[OKXContractType, ...] | None = None
    instrument_families: tuple[str, ...] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_demo: bool = False
    margin_mode: OKXMarginMode | None = None
    use_spot_margin: bool = False
    http_timeout_secs: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    use_fills_channel: bool = False
    use_mm_mass_cancel: bool = False
