# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class BinanceDataClientConfig(LiveDataClientConfig):
    """
    Configuration for ``BinanceDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    account_type : BinanceAccountType, default BinanceAccountType.SPOT
        The account type for the client.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    us : bool, default False
        If client is connecting to Binance US.
    testnet : bool, default False
        If the client is connecting to a Binance testnet.
    """

    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: Optional[str] = None
    base_url_ws: Optional[str] = None
    us: bool = False
    testnet: bool = False


class BinanceExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``BinanceExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    account_type : BinanceAccountType, default BinanceAccountType.SPOT
        The account type for the client.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_ws_http : str, optional
        The WebSocket client custom endpoint override.
    us : bool, default False
        If client is connecting to Binance US.
    testnet : bool, default False
        If the client is connecting to a Binance testnet.
    """

    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: Optional[str] = None
    base_url_ws: Optional[str] = None
    us: bool = False
    testnet: bool = False
