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
from nautilus_trader.adapters.rithmic.factories import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic.factories import RithmicLiveExecClientFactory
from nautilus_trader.adapters.rithmic.providers import RithmicInstrumentProvider


try:
    from nautilus_trader.adapters.rithmic.bindings import AccountEvent as AccountEvent
    from nautilus_trader.adapters.rithmic.bindings import ConnectionState as ConnectionState
    from nautilus_trader.adapters.rithmic.bindings import ExecutionEvent as ExecutionEvent
    from nautilus_trader.adapters.rithmic.bindings import MarketDataEvent as MarketDataEvent
    from nautilus_trader.adapters.rithmic.bindings import OrderAccepted as OrderAccepted
    from nautilus_trader.adapters.rithmic.bindings import OrderCancelled as OrderCancelled
    from nautilus_trader.adapters.rithmic.bindings import OrderFilled as OrderFilled
    from nautilus_trader.adapters.rithmic.bindings import OrderModified as OrderModified
    from nautilus_trader.adapters.rithmic.bindings import OrderRejected as OrderRejected
    from nautilus_trader.adapters.rithmic.bindings import OrderSide as OrderSide
    from nautilus_trader.adapters.rithmic.bindings import OrderStatus as OrderStatus
    from nautilus_trader.adapters.rithmic.bindings import OrderSubmitted as OrderSubmitted
    from nautilus_trader.adapters.rithmic.bindings import OrderType as OrderType
    from nautilus_trader.adapters.rithmic.bindings import PositionEvent as PositionEvent
    from nautilus_trader.adapters.rithmic.bindings import QuoteTick as QuoteTick
    from nautilus_trader.adapters.rithmic.bindings import RithmicDataClient as RithmicDataClient
    from nautilus_trader.adapters.rithmic.bindings import (
        RithmicExecutionClient as RithmicExecutionClient,
    )
    from nautilus_trader.adapters.rithmic.bindings import RithmicGateway as RithmicGateway
    from nautilus_trader.adapters.rithmic.bindings import TimeInForce as TimeInForce
    from nautilus_trader.adapters.rithmic.bindings import TradeTick as TradeTick

    _RUST_BINDINGS_AVAILABLE = True
except ImportError:
    _RUST_BINDINGS_AVAILABLE = False

__all__ = [
    "RITHMIC",
    "RITHMIC_CLIENT_ID",
    "RITHMIC_VENUE",
    "RithmicDataClientConfig",
    "RithmicEnvironment",
    "RithmicExecClientConfig",
    "RithmicInstrumentProvider",
    "RithmicLiveDataClient",
    "RithmicLiveDataClientFactory",
    "RithmicLiveExecClientFactory",
    "RithmicLiveExecutionClient",
]

if _RUST_BINDINGS_AVAILABLE:
    __all__ += [
        "AccountEvent",
        "ConnectionState",
        "ExecutionEvent",
        "MarketDataEvent",
        "OrderAccepted",
        "OrderCancelled",
        "OrderFilled",
        "OrderModified",
        "OrderRejected",
        "OrderSide",
        "OrderStatus",
        "OrderSubmitted",
        "OrderType",
        "PositionEvent",
        "QuoteTick",
        "RithmicDataClient",
        "RithmicExecutionClient",
        "RithmicGateway",
        "TimeInForce",
        "TradeTick",
    ]
