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
Rithmic futures integration adapter.

This subpackage provides configuration objects, client factories, providers,
and Python bindings for connecting NautilusTrader to Rithmic market data and
execution plants.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.rithmic``.
"""

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import RithmicEnvironment
from nautilus_trader.adapters.rithmic.config import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic.constants import RITHMIC
from nautilus_trader.adapters.rithmic.constants import RITHMIC_CLIENT_ID
from nautilus_trader.adapters.rithmic.constants import RITHMIC_VENUE
from nautilus_trader.adapters.rithmic.data import RithmicLiveDataClient
from nautilus_trader.adapters.rithmic.execution import RithmicLiveExecutionClient
from nautilus_trader.adapters.rithmic.factories import (
    RithmicLiveDataClientFactory,
    RithmicLiveExecClientFactory,
)
from nautilus_trader.adapters.rithmic.providers import RithmicInstrumentProvider

try:
    from nautilus_trader.adapters.rithmic.bindings import (
        # Gateway
        RithmicGateway,
        # Clients
        RithmicDataClient,
        RithmicExecutionClient,
        # Enums
        OrderSide,
        OrderType,
        TimeInForce,
        OrderStatus,
        ConnectionState,
        # Events
        QuoteTick,
        TradeTick,
        MarketDataEvent,
        OrderSubmitted,
        OrderAccepted,
        OrderRejected,
        OrderFilled,
        OrderCancelled,
        OrderModified,
        ExecutionEvent,
        AccountEvent,
        PositionEvent,
    )
    _RUST_BINDINGS_AVAILABLE = True
except ImportError:
    _RUST_BINDINGS_AVAILABLE = False

__all__ = [
    # Configuration
    "RithmicEnvironment",
    "RithmicDataClientConfig",
    "RithmicExecClientConfig",
    "RITHMIC",
    "RITHMIC_CLIENT_ID",
    "RITHMIC_VENUE",
    # Providers
    "RithmicInstrumentProvider",
    # High-level NautilusTrader Clients
    "RithmicLiveDataClient",
    "RithmicLiveExecutionClient",
    # Factories
    "RithmicLiveDataClientFactory",
    "RithmicLiveExecClientFactory",
]

# Add Rust bindings to __all__ if available
if _RUST_BINDINGS_AVAILABLE:
    __all__.extend([
        # Gateway
        "RithmicGateway",
        # Low-level Rust Clients
        "RithmicDataClient",
        "RithmicExecutionClient",
        # Enums
        "OrderSide",
        "OrderType",
        "TimeInForce",
        "OrderStatus",
        "ConnectionState",
        # Market Data Events
        "QuoteTick",
        "TradeTick",
        "MarketDataEvent",
        # Execution Events
        "OrderSubmitted",
        "OrderAccepted",
        "OrderRejected",
        "OrderFilled",
        "OrderCancelled",
        "OrderModified",
        "ExecutionEvent",
        "AccountEvent",
        "PositionEvent",
    ])
