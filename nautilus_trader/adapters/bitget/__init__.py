# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------
"""Bitget exchange integration adapter."""

from nautilus_trader.adapters.bitget.config import BitgetDataClientConfig
from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig
from nautilus_trader.adapters.bitget.constants import BITGET
from nautilus_trader.adapters.bitget.constants import BITGET_CLIENT_ID
from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.adapters.bitget.factories import BitgetLiveDataClientFactory
from nautilus_trader.adapters.bitget.factories import BitgetLiveExecClientFactory
from nautilus_trader.adapters.bitget.providers import BitgetInstrumentProvider


__all__ = [
    "BITGET",
    "BITGET_CLIENT_ID",
    "BITGET_VENUE",
    "BitgetDataClientConfig",
    "BitgetExecClientConfig",
    "BitgetInstrumentProvider",
    "BitgetLiveDataClientFactory",
    "BitgetLiveExecClientFactory",
]
