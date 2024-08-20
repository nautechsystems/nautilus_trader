# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

    testnet : bool, default False
        If the client is connecting to the dYdX testnet API.

    """

    wallet_address: str | None = None
    is_testnet: bool = False


class DYDXExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BybitExecutionClient`` instances.

    testnet : bool, default False
        If the client is connecting to the Bybit testnet API.
    use_gtd : bool, default False
        If False then GTD time in force will be remapped to GTC
        (this is useful if managing GTD orders locally).

    """

    wallet_address: str | None = None
    subaccount: int = 0
    mnemonic: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    is_testnet: bool = False
    use_gtd: bool = False
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
