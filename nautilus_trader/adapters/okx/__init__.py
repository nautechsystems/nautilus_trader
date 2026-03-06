"""
OKX cryptocurreny exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, and constants for connecting to and interacting with OKX's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.okx``.

"""

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX
from nautilus_trader.adapters.okx.constants import OKX_CLIENT_ID
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.factories import OKXLiveDataClientFactory
from nautilus_trader.adapters.okx.factories import OKXLiveExecClientFactory
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider


__all__ = [
    "OKX",
    "OKX_CLIENT_ID",
    "OKX_VENUE",
    "OKXDataClientConfig",
    "OKXExecClientConfig",
    "OKXInstrumentProvider",
    "OKXLiveDataClientFactory",
    "OKXLiveExecClientFactory",
]
