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
Kraken cryptocurrency exchange integration adapter.

This subpackage provides an instrument provider, data client, execution client,
configurations, and constants for connecting to and interacting with Kraken's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.kraken``.

"""

from nautilus_trader.adapters.kraken.config import KrakenDataClientConfig
from nautilus_trader.adapters.kraken.config import KrakenExecClientConfig
from nautilus_trader.adapters.kraken.constants import KRAKEN
from nautilus_trader.adapters.kraken.constants import KRAKEN_CLIENT_ID
from nautilus_trader.adapters.kraken.constants import KRAKEN_VENUE
from nautilus_trader.adapters.kraken.data import KrakenDataClient
from nautilus_trader.adapters.kraken.execution import KrakenExecutionClient
from nautilus_trader.adapters.kraken.factories import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken.factories import KrakenLiveExecClientFactory
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.adapters.kraken.types import KRAKEN_INSTRUMENT_TYPES
from nautilus_trader.adapters.kraken.types import KrakenInstrument
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType


__all__ = [
    "KRAKEN",
    "KRAKEN_CLIENT_ID",
    "KRAKEN_INSTRUMENT_TYPES",
    "KRAKEN_VENUE",
    "KrakenDataClient",
    "KrakenDataClientConfig",
    "KrakenEnvironment",
    "KrakenExecClientConfig",
    "KrakenExecutionClient",
    "KrakenInstrument",
    "KrakenInstrumentProvider",
    "KrakenLiveDataClientFactory",
    "KrakenLiveExecClientFactory",
    "KrakenProductType",
]
