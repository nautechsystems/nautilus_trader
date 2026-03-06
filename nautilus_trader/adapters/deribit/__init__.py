"""
The Deribit adapter provides integration with the Deribit cryptocurrency derivatives
exchange, supporting live market data ingest and order execution.

This adapter supports:
- Market data streaming via WebSocket (trades, order book, quotes).
- Order execution via WebSocket (market, limit, stop market, stop limit).
- Instrument definitions for futures, options, spot, and combo instruments.
- Multiple currencies (BTC, ETH, USDC, USDT, EURR).

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
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType
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
    "DeribitInstrumentProvider",
    "DeribitLiveDataClientFactory",
    "DeribitLiveExecClientFactory",
    "DeribitProductType",
    "DeribitUpdateInterval",
    "DeribitWebSocketClient",
    "get_cached_deribit_http_client",
    "get_cached_deribit_instrument_provider",
]
