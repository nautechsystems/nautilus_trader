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

from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


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
    passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], optional
        The OKX instrument types of instruments to load. The default is `(OKXInstrumentType.SWAP,)`.
        If None, all instrument types are loaded (subject to contract types and their compatibility
        with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load. The default is `(OKXInstrumentType.LINEAR,)`.
        If None, all contract types are loaded (subject to instrument types and their compatibility
        with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `get_http_base_url()`.
    base_url_ws : str, optional
        The base url to OKX's websocket api.
        If ``None`` then will source the url from `get_ws_base_url()`.
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.

    """

    api_key: str | None = None
    api_secret: str | None = None
    passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType] | None = (OKXInstrumentType.SWAP,)
    contract_types: tuple[OKXContractType] | None = (OKXContractType.LINEAR,)
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_demo: bool = False
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
    passphrase : str, [default=None]
        The passphrase used when creating the OKX API keys.
        If ``None`` then will source the `OKX_PASSPHRASE` environment variable.
    instrument_types : tuple[OKXInstrumentType], optional
        The OKX instrument types of instruments to load. The default is `(OKXInstrumentType.SWAP,)`.
        If None, all instrument types are loaded (subject to contract types and their compatibility
        with instrument types).
    contract_types : tuple[OKXInstrumentType], optional
        The OKX contract types of instruments to load. The default is `(OKXInstrumentType.LINEAR,)`.
        If None, all contract types are loaded (subject to instrument types and their compatibility
        with contract types).
    base_url_http : str, optional
        The base url to OKX's http api.
        If ``None`` then will source the `get_http_base_url()`.
    base_url_ws : str, optional
        The base url to OKX's websocket api.
        If ``None`` then will source the url from `get_ws_base_url()`.
    margin_mode : OKXMarginMode, [default=OKXMarginMode.CROSS]
        The intended OKX account margin mode (referred to as mgnMode by OKX's docs).
    is_demo : bool, default False
        If the client is connecting to the OKX demo API.

    """

    api_key: str | None = None
    api_secret: str | None = None
    passphrase: str | None = None
    instrument_types: tuple[OKXInstrumentType] | None = (OKXInstrumentType.SWAP,)
    contract_types: tuple[OKXContractType] | None = (OKXContractType.LINEAR,)
    base_url_http: str | None = None
    base_url_ws: str | None = None
    margin_mode: OKXMarginMode = OKXMarginMode.CROSS
    is_demo: bool = False
    # use_reduce_only: bool = True  # TODO: check if applicable -> taken from Bybit
