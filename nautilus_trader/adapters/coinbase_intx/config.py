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

from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_VENUE
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.identifiers import Venue


class CoinbaseIntxDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``CoinbaseIntxDataClient`` instances.

    Parameters
    ----------
    venue : Venue, default COINBASE_INTX_VENUE
        The venue for the client.
    api_key : str, optional
        The Coinbase International API public key.
        If ``None`` then will source the `COINBASE_INTX_API_KEY` or
        `COINBASE_INTX_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Coinbase International API private key.
        If ``None`` then will source the `COINBASE_INTX_API_SECRET` or
        `COINBASE_INTX_TESTNET_API_SECRET` environment variables.
    api_passphrase : str, optional
        The Coinbase International API public key.
        If ``None`` then will source the `COINBASE_INTX_API_PASSPHRASE` or
        `COINBASE_INTX_TESTNET_API_PASSPHRASE` environment variables.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    http_timeout_secs : PositiveInt or None, default 60
        The default timeout (seconds) for HTTP requests.

    """

    venue: Venue = COINBASE_INTX_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt | None = 60


class CoinbaseIntxExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``CoinbaseIntxExecClient`` instances.

    Parameters
    ----------
    venue : Venue, default COINBASE_INTX_VENUE
        The venue for the client.
    api_key : str, optional
        The Coinbase International API public key.
        If ``None`` then will source the `COINBASE_INTX_API_KEY` or
        `COINBASE_INTX_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Coinbase International API private key.
        If ``None`` then will source the `COINBASE_INTX_API_SECRET` or
        `COINBASE_INTX_TESTNET_API_SECRET` environment variables.
    api_passphrase : str, optional
        The Coinbase International API public key.
        If ``None`` then will source the `COINBASE_INTX_API_PASSPHRASE` or
        `COINBASE_INTX_TESTNET_API_PASSPHRASE` environment variables.
    portfolio_id : str, optional
        The Coinbase International portfolio to be traded.
        If ``None`` then will source the `COINBASE_INTX_PORTFOLIO_ID` environment variable.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    http_timeout_secs : PositiveInt or None, default 60
        The default timeout (seconds) for HTTP requests.

    """

    venue: Venue = COINBASE_INTX_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    portfolio_id: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt | None = 60
