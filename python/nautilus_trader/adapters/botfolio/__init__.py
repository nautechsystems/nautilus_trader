# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------
"""
Botfolio local paper trading adapter for Nautilus Trader.

Provides data and execution clients for local paper trading with:
- Price data from EODHD via Redis pub/sub
- Simulated order execution with configurable slippage/latency
- Position tracking via the bot-folio backend

This adapter allows strategies to be tested with realistic fill simulation
without requiring a live brokerage connection.

Environment Variables
---------------------
REDIS_URL : str
    Redis connection URL (default: redis://localhost:6379).
BOTFOLIO_BOT_ID : str
    Bot ID for event emission (used by EventEmitter actor).

Example
-------
>>> from nautilus_trader.adapters.botfolio import BOTFOLIO
>>> from nautilus_trader.adapters.botfolio import BotfolioDataClientConfig
>>> from nautilus_trader.adapters.botfolio import BotfolioExecClientConfig
>>> from nautilus_trader.adapters.botfolio import BotfolioLiveDataClientFactory
>>> from nautilus_trader.adapters.botfolio import BotfolioLiveExecClientFactory
>>> from nautilus_trader.config import TradingNodeConfig
>>>
>>> config = TradingNodeConfig(
...     data_clients={
...         BOTFOLIO: BotfolioDataClientConfig(
...             redis_url="redis://localhost:6379",
...             symbols=["BTC-USD", "ETH-USD"],
...         ),
...     },
...     exec_clients={
...         BOTFOLIO: BotfolioExecClientConfig(
...             redis_url="redis://localhost:6379",
...             starting_balance="100000 USD",
...         ),
...     },
... )

"""

from nautilus_trader.adapters.botfolio.config import BotfolioDataClientConfig
from nautilus_trader.adapters.botfolio.config import BotfolioExecClientConfig
from nautilus_trader.adapters.botfolio.constants import BOTFOLIO_VENUE
from nautilus_trader.adapters.botfolio.data import BotfolioDataClient
from nautilus_trader.adapters.botfolio.execution import BotfolioExecutionClient
from nautilus_trader.adapters.botfolio.factories import BotfolioLiveDataClientFactory
from nautilus_trader.adapters.botfolio.factories import BotfolioLiveExecClientFactory
from nautilus_trader.adapters.botfolio.fill_model import BotfolioFillModel
from nautilus_trader.adapters.botfolio.providers import BotfolioInstrumentProvider


# Convenience alias
BOTFOLIO = "BOTFOLIO"

__all__ = [
    "BOTFOLIO",
    "BOTFOLIO_VENUE",
    "BotfolioDataClient",
    "BotfolioDataClientConfig",
    "BotfolioExecClientConfig",
    "BotfolioExecutionClient",
    "BotfolioFillModel",
    "BotfolioInstrumentProvider",
    "BotfolioLiveDataClientFactory",
    "BotfolioLiveExecClientFactory",
]

