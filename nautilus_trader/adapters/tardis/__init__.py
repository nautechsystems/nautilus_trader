"""
Tardis crypto market data integration adapter https://tardis.dev/.

This subpackage provides instrument providers, data client configuration,
factories, constants, and data loaders for connecting to and interacting with
Tardis APIs and the Tardis Machine server.

For convenience, the most commonly used symbols are re-exported at the subpackage's
top level, so downstream code can simply import from ``nautilus_trader.adapters.tardis``.

"""

from nautilus_trader.adapters.tardis.config import TardisDataClientConfig
from nautilus_trader.adapters.tardis.constants import TARDIS
from nautilus_trader.adapters.tardis.constants import TARDIS_CLIENT_ID
from nautilus_trader.adapters.tardis.factories import TardisLiveDataClientFactory
from nautilus_trader.adapters.tardis.factories import get_tardis_http_client
from nautilus_trader.adapters.tardis.factories import get_tardis_instrument_provider
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.adapters.tardis.providers import TardisInstrumentProvider


__all__ = [
    "TARDIS",
    "TARDIS_CLIENT_ID",
    "TardisCSVDataLoader",
    "TardisDataClientConfig",
    "TardisInstrumentProvider",
    "TardisLiveDataClientFactory",
    "get_tardis_http_client",
    "get_tardis_instrument_provider",
]
