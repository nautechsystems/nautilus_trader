# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------
"""
Alpaca adapter for Nautilus Trader.

Provides data and execution clients for trading US equities via Alpaca.

Supports:
- API key authentication (APCA-API-KEY-ID / APCA-API-SECRET-KEY)
- OAuth authentication (Bearer token)
- Paper and live trading
- Market data streaming (quotes, trades, bars)
- Order execution (market, limit, stop, stop-limit)
- Position and account management

Environment Variables
---------------------
APCA_API_KEY_ID : str
    Alpaca API key (for API key auth).
APCA_API_SECRET_KEY : str
    Alpaca API secret (for API key auth).
APCA_API_ACCESS_TOKEN : str
    Alpaca OAuth access token (for OAuth auth).
APCA_API_BASE_URL : str
    Alpaca API base URL (auto-detected from paper setting if not set).

Example
-------
>>> from nautilus_trader.adapters.alpaca import ALPACA
>>> from nautilus_trader.adapters.alpaca import AlpacaDataClientConfig
>>> from nautilus_trader.adapters.alpaca import AlpacaExecClientConfig
>>> from nautilus_trader.adapters.alpaca import AlpacaLiveDataClientFactory
>>> from nautilus_trader.adapters.alpaca import AlpacaLiveExecClientFactory
>>> from nautilus_trader.config import TradingNodeConfig
>>>
>>> config = TradingNodeConfig(
...     data_clients={
...         ALPACA: AlpacaDataClientConfig(paper=True),
...     },
...     exec_clients={
...         ALPACA: AlpacaExecClientConfig(paper=True),
...     },
... )

"""

from nautilus_trader.adapters.alpaca.config import AlpacaDataClientConfig
from nautilus_trader.adapters.alpaca.config import AlpacaExecClientConfig
from nautilus_trader.adapters.alpaca.constants import ALPACA_VENUE
from nautilus_trader.adapters.alpaca.constants import AlpacaAssetClass
from nautilus_trader.adapters.alpaca.constants import AlpacaAssetStatus
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderSide
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderStatus
from nautilus_trader.adapters.alpaca.constants import AlpacaOrderType
from nautilus_trader.adapters.alpaca.constants import AlpacaTimeInForce
from nautilus_trader.adapters.alpaca.data import AlpacaDataClient
from nautilus_trader.adapters.alpaca.execution import AlpacaExecutionClient
from nautilus_trader.adapters.alpaca.factories import AlpacaLiveDataClientFactory
from nautilus_trader.adapters.alpaca.factories import AlpacaLiveExecClientFactory
from nautilus_trader.adapters.alpaca.http.client import AlpacaHttpClient
from nautilus_trader.adapters.alpaca.providers import AlpacaInstrumentProvider


# Convenience alias
ALPACA = "ALPACA"

__all__ = [
    "ALPACA",
    "ALPACA_VENUE",
    "AlpacaAssetClass",
    "AlpacaAssetStatus",
    "AlpacaDataClient",
    "AlpacaDataClientConfig",
    "AlpacaExecClientConfig",
    "AlpacaExecutionClient",
    "AlpacaHttpClient",
    "AlpacaInstrumentProvider",
    "AlpacaLiveDataClientFactory",
    "AlpacaLiveExecClientFactory",
    "AlpacaOrderSide",
    "AlpacaOrderStatus",
    "AlpacaOrderType",
    "AlpacaTimeInForce",
]

