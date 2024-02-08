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


from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt


class BybitDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BybitDataClient`` instances.
    """

    api_key: str | None = None
    api_secret: str | None = None
    instrument_types: list[BybitInstrumentType] = []
    base_url_http: str | None = None
    testnet: bool = False


class BybitExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BybitExecutionClient`` instances.
    """

    api_key: str | None = None
    api_secret: str | None = None
    instrument_types: list[BybitInstrumentType] = []
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    clock_sync_interval_secs: int = 0
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
