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
Delta Exchange adapter for Nautilus Trader.

This adapter provides integration with Delta Exchange, a derivatives trading platform
that offers perpetual futures and options trading.
"""

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_CLIENT_ID
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_VENUE
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeOrderSide
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeOrderStatus
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeOrderType
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeProductType
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeTimeInForce
from nautilus_trader.adapters.delta_exchange.constants import DeltaExchangeTradingStatus
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.adapters.delta_exchange.factories import DeltaExchangeLiveDataClientFactory
from nautilus_trader.adapters.delta_exchange.factories import DeltaExchangeLiveExecClientFactory
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider


__all__ = [
    # Configuration classes
    "DeltaExchangeDataClientConfig",
    "DeltaExchangeExecClientConfig",
    # Client classes
    "DeltaExchangeDataClient",
    "DeltaExchangeExecutionClient",
    # Factory classes
    "DeltaExchangeLiveDataClientFactory",
    "DeltaExchangeLiveExecClientFactory",
    # Provider classes
    "DeltaExchangeInstrumentProvider",
    # Constants and identifiers
    "DELTA_EXCHANGE",
    "DELTA_EXCHANGE_VENUE",
    "DELTA_EXCHANGE_CLIENT_ID",
    # Enumerations
    "DeltaExchangeProductType",
    "DeltaExchangeOrderType",
    "DeltaExchangeOrderStatus",
    "DeltaExchangeTimeInForce",
    "DeltaExchangeOrderSide",
    "DeltaExchangeTradingStatus",
]
