"""
Betfair sports betting exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, data types and constants for connecting to and interacting with
Betfairs's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.betfair``.

"""

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR
from nautilus_trader.adapters.betfair.constants import BETFAIR_CLIENT_ID
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_client
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_instrument_provider
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig


__all__ = [
    "BETFAIR",
    "BETFAIR_CLIENT_ID",
    "BETFAIR_VENUE",
    "BetfairDataClientConfig",
    "BetfairExecClientConfig",
    "BetfairInstrumentProvider",
    "BetfairInstrumentProviderConfig",
    "BetfairLiveDataClientFactory",
    "BetfairLiveExecClientFactory",
    "BetfairParser",
    "get_cached_betfair_client",
    "get_cached_betfair_instrument_provider",
]
