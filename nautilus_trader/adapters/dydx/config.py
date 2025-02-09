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
"""
Define the dYdX configuration classes.
"""

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt


class DYDXDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``DYDXDataClient`` instances.

    Parameters
    ----------
    wallet_address : str, optional
        The dYdX wallet address.
        If ``None`` then will source `DYDX_WALLET_ADDRESS` or
        `DYDX_TESTNET_WALLET_ADDRESS` environment variables.
    testnet : bool, default False
        If the client is connecting to the dYdX testnet API.
    update_instruments_interval_mins : PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    max_ws_send_retries : int, optional
        Maximum retries when sending websocket messages.
    max_ws_retry_delay_secs : float, optional
        The delay (seconds) between retry attempts when resending websocket messages.

    """

    wallet_address: str | None = None
    is_testnet: bool = False
    update_instruments_interval_mins: PositiveInt | None = 60
    max_ws_send_retries: PositiveInt | None = None
    max_ws_retry_delay_secs: PositiveFloat | None = None


class DYDXExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``DYDXExecutionClient`` instances.

    Parameters
    ----------
    wallet_address : str, optional
        The dYdX wallet address.
        If ``None`` then will source `DYDX_WALLET_ADDRESS` or
        `DYDX_TESTNET_WALLET_ADDRESS` environment variables.
    subaccount : int, optional
        The subaccount number.
        The venue creates subaccount 0 by default.
    mnemonic : str, optional
        The mnemonic string which is used to generate the private key.
        The private key is used to sign transactions like submitting orders.
        If ``None`` then will source `DYDX_MNEMONIC` or
        `DYDX_TESTNET_MNEMONIC` environment variables.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    is_testnet : bool, default False
        If the client is connecting to the dYdX testnet API.
    max_retries : PositiveInt, optional
        The maximum number of times a submit, cancel or modify order request will be retried.
    retry_delay : PositiveFloat, optional
        The delay (seconds) between retries. Short delays with frequent retries may result in account bans.

    """

    wallet_address: str | None = None
    subaccount: int = 0
    mnemonic: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_testnet: bool = False
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
