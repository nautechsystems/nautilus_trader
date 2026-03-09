"""
Polymarket decentralized prediction market integration adapter.

This subpackage provides instrument providers, data and execution client configurations,
factories, constants, and credential helpers for connecting to and interacting with
the Polymarket Central Limit Order Book (CLOB) API.

For convenience, the most commonly used symbols are re-exported at the subpackage's
top level, so downstream code can simply import from ``nautilus_trader.adapters.polymarket``.

"""

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_CLIENT_ID
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRECISION_MAKER
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRECISION_TAKER
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.config import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket.factories import PolymarketLiveDataClientFactory
from nautilus_trader.adapters.polymarket.factories import PolymarketLiveExecClientFactory
from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client
from nautilus_trader.adapters.polymarket.factories import get_polymarket_instrument_provider
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider


__all__ = [
    "POLYMARKET",
    "POLYMARKET_CLIENT_ID",
    "POLYMARKET_MAX_PRECISION_MAKER",
    "POLYMARKET_MAX_PRECISION_TAKER",
    "POLYMARKET_MAX_PRICE",
    "POLYMARKET_MIN_PRICE",
    "POLYMARKET_VENUE",
    "PolymarketDataClientConfig",
    "PolymarketDataLoader",
    "PolymarketExecClientConfig",
    "PolymarketInstrumentProvider",
    "PolymarketLiveDataClientFactory",
    "PolymarketLiveExecClientFactory",
    "get_polymarket_http_client",
    "get_polymarket_instrument_id",
    "get_polymarket_instrument_provider",
    "parse_polymarket_instrument",
]
