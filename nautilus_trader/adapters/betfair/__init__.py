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
Betfair sports betting exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, data types and constants for connecting to and interacting with
Betfairs's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.betfair``.

"""
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR
from nautilus_trader.adapters.betfair.constants import BETFAIR_CLIENT_ID
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_client
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_instrument_provider
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig


__all__ = [
    "BETFAIR",
    "BETFAIR_CLIENT_ID",
    "BETFAIR_VENUE",
    "BetfairDataClientConfig",
    "BetfairExecClientConfig",
    "BetfairInstrumentProvider",
    "BetfairInstrumentProviderConfig",
    "BetfairLiveDataClientFactory",
    "BetfairLiveExecClientFactory",
    "BetfairParser",
    "get_cached_betfair_client",
    "get_cached_betfair_instrument_provider",
]
