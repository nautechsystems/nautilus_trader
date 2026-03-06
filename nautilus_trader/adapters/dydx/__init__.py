"""
DYdX v4 cryptocurrency exchange adapter (Rust-backed implementation).

The v4 adapter uses Rust-backed HTTP, WebSocket, and gRPC clients for:
- Native Cosmos SDK transaction signing via Rust
- Direct validator node communication
- Improved performance and reliability
- Real-time market data streaming

Usage:
    from nautilus_trader.adapters.dydx import DydxDataClientConfig
    from nautilus_trader.adapters.dydx import DydxExecClientConfig
    from nautilus_trader.adapters.dydx import DydxLiveDataClientFactory
    from nautilus_trader.adapters.dydx import DydxLiveExecClientFactory

"""

from nautilus_trader.adapters.dydx.config import DydxDataClientConfig
from nautilus_trader.adapters.dydx.config import DydxExecClientConfig
from nautilus_trader.adapters.dydx.constants import DYDX
from nautilus_trader.adapters.dydx.constants import DYDX_CLIENT_ID
from nautilus_trader.adapters.dydx.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.data import DydxDataClient
from nautilus_trader.adapters.dydx.execution import DydxExecutionClient
from nautilus_trader.adapters.dydx.factories import DydxLiveDataClientFactory
from nautilus_trader.adapters.dydx.factories import DydxLiveExecClientFactory
from nautilus_trader.adapters.dydx.providers import DydxInstrumentProvider


__all__ = [
    "DYDX",
    "DYDX_CLIENT_ID",
    "DYDX_VENUE",
    "DydxDataClient",
    "DydxDataClientConfig",
    "DydxExecClientConfig",
    "DydxExecutionClient",
    "DydxInstrumentProvider",
    "DydxLiveDataClientFactory",
    "DydxLiveExecClientFactory",
]
