"""
Binance cryptocurreny exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, data types and constants for connecting to and interacting with
Binance's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.binance``.

"""

from typing import Final

import pyarrow as pa

from nautilus_trader.adapters.binance.common.constants import BINANCE
from nautilus_trader.adapters.binance.common.constants import BINANCE_CLIENT_ID
from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceKeyType
from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily
from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.config import BinanceInstrumentProviderConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.serialization import register_serializable_type
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA
from nautilus_trader.serialization.arrow.serializer import make_dict_deserializer
from nautilus_trader.serialization.arrow.serializer import make_dict_serializer
from nautilus_trader.serialization.arrow.serializer import register_arrow


register_serializable_type(
    BinanceBar,
    BinanceBar.to_dict,
    BinanceBar.from_dict,
)

register_serializable_type(
    BinanceTicker,
    BinanceTicker.to_dict,
    BinanceTicker.from_dict,
)

BINANCE_BAR_ARROW_SCHEMA: Final[pa.schema] = pa.schema(
    {
        "bar_type": pa.dictionary(pa.int16(), pa.string()),
        "instrument_id": pa.dictionary(pa.int64(), pa.string()),
        "open": pa.string(),
        "high": pa.string(),
        "low": pa.string(),
        "close": pa.string(),
        "volume": pa.string(),
        "quote_volume": pa.string(),
        "count": pa.uint64(),
        "taker_buy_base_volume": pa.string(),
        "taker_buy_quote_volume": pa.string(),
        "ts_event": pa.uint64(),
        "ts_init": pa.uint64(),
    },
)

NAUTILUS_ARROW_SCHEMA[BinanceBar] = BINANCE_BAR_ARROW_SCHEMA

register_arrow(
    BinanceBar,
    BINANCE_BAR_ARROW_SCHEMA,
    encoder=make_dict_serializer(BINANCE_BAR_ARROW_SCHEMA),
    decoder=make_dict_deserializer(BinanceBar),
)

__all__ = [
    "BINANCE",
    "BINANCE_CLIENT_ID",
    "BINANCE_VENUE",
    "BinanceAccountType",
    "BinanceDataClientConfig",
    "BinanceExecClientConfig",
    "BinanceFuturesInstrumentProvider",
    "BinanceFuturesMarkPriceUpdate",
    "BinanceInstrumentProviderConfig",
    "BinanceKeyType",
    "BinancePrivateApiFamily",
    "BinanceLiveDataClientFactory",
    "BinanceLiveExecClientFactory",
    "BinanceOrderBookDeltaDataLoader",
    "BinanceSpotInstrumentProvider",
    "get_cached_binance_http_client",
]
