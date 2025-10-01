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
from __future__ import annotations

from nautilus_trader.adapters.schwab.common import SCHWAB_VENUE
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import NautilusConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.identifiers import Venue


class SchwabClientConfig(NautilusConfig, frozen=True):
    """
    Connection details required to create a ``schwab-py`` client.
    """

    api_key: str | None = None
    app_secret: str | None = None
    callback_url: str | None = None
    token_path: str | None = None


class SchwabInstrumentProviderConfig(InstrumentProviderConfig, frozen=True):
    """
    Instrument bootstrap configuration for Schwab universes.
    """

    equity_exchange: str = "XNAS"
    equity_currency: str = "USD"
    equity_price_precision: int = 2
    equity_tick_size: PositiveFloat = 0.01
    equity_lot_size: PositiveInt = 1
    option_exchange: str = "OPRA"
    option_currency: str = "USD"
    option_price_precision: int = 2
    option_tick_size: PositiveFloat = 0.01
    option_multiplier: PositiveFloat = 100.0


class SchwabDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for :class:`SchwabDataClient` instances.
    """

    venue: Venue = SCHWAB_VENUE
    account_id: str | None = None
    http_client: SchwabClientConfig | None = None
    include_pre_market: bool = False
    bars_timestamp_on_close: bool = True


class SchwabExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for :class:`SchwabExecutionClient` instances.
    """

    venue: Venue = SCHWAB_VENUE
    account_number: str = ""
    http_client: SchwabClientConfig | None = None
    session: str = "NORMAL"
    default_duration: str = "DAY"
    default_instruction_equity_buy: str = "BUY"
    default_instruction_equity_sell: str = "SELL"
    default_instruction_option_buy: str = "BUY_TO_OPEN"
    default_instruction_option_sell: str = "SELL_TO_CLOSE"
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None


__all__ = [
    "SchwabClientConfig",
    "SchwabDataClientConfig",
    "SchwabExecClientConfig",
    "SchwabInstrumentProviderConfig",
]
