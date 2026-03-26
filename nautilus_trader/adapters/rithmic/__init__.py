"""
NautilusTrader adapter for Rithmic futures trading.

This package provides connectivity to Rithmic's R | Protocol API™
for market data, instruments, and in-progress execution integration
on futures exchanges.

Example
-------
>>> from nautilus_trader.adapters.rithmic import (
...     RithmicDataClientConfig,
...     RithmicLiveDataClient,
...     RithmicLiveDataClientFactory,
... )
>>>
>>> config = RithmicDataClientConfig(
...     environment=RithmicEnvironment.DEMO,
...     username="your_username",
...     password="your_password",
...     system_name="your_system",
... )
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

# Try to import Rust bindings (may not be available if extension not built)
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

__version__ = "0.1.0"

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
