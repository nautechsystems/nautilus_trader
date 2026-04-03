# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
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
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.serialization import register_serializable_type
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA
from nautilus_trader.serialization.arrow.serializer import make_dict_deserializer
from nautilus_trader.serialization.arrow.serializer import make_dict_serializer
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.arrow.serializer import register_rust_custom_serializer


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


_binance_mod = nautilus_pyo3.binance  # type: ignore[attr-defined]


def _convert_binance_bar_to_pyo3(bar: BinanceBar) -> object:
    return _binance_mod.BinanceBar.from_dict(BinanceBar.to_dict(bar))


register_rust_custom_serializer(
    "BinanceBar",
    _binance_mod.binance_bar_to_arrow_record_batch_bytes,
    _convert_binance_bar_to_pyo3,
    data_cls=BinanceBar,
)


BINANCE_FUTURES_MARK_PRICE_UPDATE_ARROW_SCHEMA: Final[pa.schema] = pa.schema(
    {
        "instrument_id": pa.dictionary(pa.int64(), pa.string()),
        "mark": pa.string(),
        "index": pa.string(),
        "estimated_settle": pa.string(),
        "funding_rate": pa.string(),
        "next_funding_ns": pa.uint64(),
        "ts_event": pa.uint64(),
        "ts_init": pa.uint64(),
    },
)

NAUTILUS_ARROW_SCHEMA[BinanceFuturesMarkPriceUpdate] = (
    BINANCE_FUTURES_MARK_PRICE_UPDATE_ARROW_SCHEMA
)

register_arrow(
    BinanceFuturesMarkPriceUpdate,
    BINANCE_FUTURES_MARK_PRICE_UPDATE_ARROW_SCHEMA,
    encoder=make_dict_serializer(BINANCE_FUTURES_MARK_PRICE_UPDATE_ARROW_SCHEMA),
    decoder=make_dict_deserializer(BinanceFuturesMarkPriceUpdate),
)

decode_binance_spot_client_order_id = nautilus_pyo3.binance.decode_binance_spot_client_order_id  # type: ignore[attr-defined]
decode_binance_futures_client_order_id = (
    nautilus_pyo3.binance.decode_binance_futures_client_order_id  # type: ignore[attr-defined]
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
    "BinanceLiveDataClientFactory",
    "BinanceLiveExecClientFactory",
    "BinanceOrderBookDeltaDataLoader",
    "BinanceSpotInstrumentProvider",
    "decode_binance_futures_client_order_id",
    "decode_binance_spot_client_order_id",
    "get_cached_binance_http_client",
]
