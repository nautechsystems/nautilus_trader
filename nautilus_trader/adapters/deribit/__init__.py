# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
The Deribit adapter provides integration with the Deribit cryptocurrency derivatives
exchange.

This adapter supports:
- Market data streaming via WebSocket (trades, order book, quotes)
- Instrument definitions for futures, options, and perpetuals
- Multiple currencies (BTC, ETH, USDC, USDT, EURR)

"""

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.config import DeribitExecClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT
from nautilus_trader.adapters.deribit.constants import DERIBIT_CLIENT_ID
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.data import DeribitDataClient
from nautilus_trader.adapters.deribit.execution import DeribitExecutionClient
from nautilus_trader.adapters.deribit.factories import DeribitLiveDataClientFactory
from nautilus_trader.adapters.deribit.factories import DeribitLiveExecClientFactory
from nautilus_trader.adapters.deribit.factories import get_cached_deribit_http_client
from nautilus_trader.adapters.deribit.factories import get_cached_deribit_instrument_provider
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.core.nautilus_pyo3 import DeribitCurrency
from nautilus_trader.core.nautilus_pyo3 import DeribitHttpClient
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.core.nautilus_pyo3 import DeribitUpdateInterval
from nautilus_trader.core.nautilus_pyo3 import DeribitWebSocketClient


__all__ = [
    "DERIBIT",
    "DERIBIT_CLIENT_ID",
    "DERIBIT_VENUE",
    "DeribitCurrency",
    "DeribitDataClient",
    "DeribitDataClientConfig",
    "DeribitExecClientConfig",
    "DeribitExecutionClient",
    "DeribitHttpClient",
    "DeribitInstrumentKind",
    "DeribitInstrumentProvider",
    "DeribitLiveDataClientFactory",
    "DeribitLiveExecClientFactory",
    "DeribitUpdateInterval",
    "DeribitWebSocketClient",
    "get_cached_deribit_http_client",
    "get_cached_deribit_instrument_provider",
]
