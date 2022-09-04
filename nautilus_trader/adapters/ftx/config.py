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

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class FTXDataClientConfig(LiveDataClientConfig):
    """
    Configuration for ``FTXDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The FTX API public key.
        If ``None`` then will source the `FTX_API_KEY` environment variable
    api_secret : str, optional
        The FTX API public key.
        If ``None`` then will source the `FTX_API_KEY` environment variable.
    subaccount : str, optional
        The account type for the client.
    us : bool, default False
        If client is connecting to Binance US.
    override_usd : bool, default False
        If the built-in USD currency should be overridden with the FTX version
        which uses a precision of 8.
    """

    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    subaccount: str = None
    us: bool = False
    override_usd: bool = False


class FTXExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``FTXExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The FTX API public key.
        If ``None`` then will source the `FTX_API_KEY` environment variable
    api_secret : str, optional
        The FTX API public key.
        If ``None`` then will source the `FTX_API_KEY` environment variable.
    subaccount : str, optional
        The account type for the client.
        If ``None`` then will source the `"FTX_SUBACCOUNT"` environment variable.
    us : bool, default False
        If client is connecting to Binance US.
    account_polling_interval : int
        The interval between polling account status (seconds).
    calculated_account : bool, default False
        If account status is calculated from executions.
    override_usd : bool, default False
        If the built-in USD currency should be overridden with the FTX version
        which uses a precision of 8.
    """

    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    subaccount: str = None
    us: bool = False
    account_polling_interval: int = 60
    calculated_account = False
    override_usd: bool = False
