"""
BitMEX cryptocurrency exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, and constants for connecting to and interacting with BitMEX's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.bitmex``.

"""

from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX
from nautilus_trader.adapters.bitmex.constants import BITMEX_CLIENT_ID
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.factories import BitmexLiveDataClientFactory
from nautilus_trader.adapters.bitmex.factories import BitmexLiveExecClientFactory
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider


__all__ = [
    "BITMEX",
    "BITMEX_CLIENT_ID",
    "BITMEX_VENUE",
    "BitmexDataClientConfig",
    "BitmexExecClientConfig",
    "BitmexInstrumentProvider",
    "BitmexLiveDataClientFactory",
    "BitmexLiveExecClientFactory",
]
