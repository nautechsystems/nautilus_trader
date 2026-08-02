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
Hyperliquid blockchain integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, and constants for connecting to and interacting with Hyperliquid's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.hyperliquid``.

"""

from typing import Final

import pyarrow as pa

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_CLIENT_ID
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.data import HyperliquidAllDexsAssetCtxs
from nautilus_trader.adapters.hyperliquid.data import HyperliquidAllMids
from nautilus_trader.adapters.hyperliquid.data import HyperliquidDexAssetCtx
from nautilus_trader.adapters.hyperliquid.data import HyperliquidImpactPrices
from nautilus_trader.adapters.hyperliquid.data import HyperliquidOpenInterest
from nautilus_trader.adapters.hyperliquid.data import HyperliquidPublicTrade
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.factories import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid.factories import HyperliquidLiveExecClientFactory
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import CustomData
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.arrow.serializer import register_rust_custom_serializer


_hyperliquid_mod = nautilus_pyo3.hyperliquid  # type: ignore[attr-defined]


def _convert_hyperliquid_public_trade_to_pyo3(
    trade: HyperliquidPublicTrade | CustomData,
) -> object:
    if isinstance(trade, CustomData):
        trade = trade.data

    if not isinstance(trade, HyperliquidPublicTrade):
        raise TypeError(f"Expected HyperliquidPublicTrade, was {type(trade).__name__}")

    return trade.to_pyo3()


def _decode_hyperliquid_public_trades(
    table: pa.Table | pa.RecordBatch,
) -> list[HyperliquidPublicTrade]:
    batches = table.to_batches() if isinstance(table, pa.Table) else [table]
    return [
        HyperliquidPublicTrade.from_pyo3(trade)
        for batch in batches
        for trade in _hyperliquid_mod.HyperliquidPublicTrade.decode_record_batch_py({}, batch)
    ]


register_rust_custom_serializer(
    "HyperliquidPublicTrade",
    _hyperliquid_mod.HyperliquidPublicTrade.to_arrow_record_batch_bytes,
    _convert_hyperliquid_public_trade_to_pyo3,
    data_cls=HyperliquidPublicTrade,
)


HYPERLIQUID_PUBLIC_TRADE_ARROW_SCHEMA: Final[pa.Schema] = pa.schema(
    [
        pa.field("instrument_id", pa.string(), nullable=False),
        pa.field("price", pa.string(), nullable=False),
        pa.field("size", pa.string(), nullable=False),
        pa.field("aggressor_side", pa.string(), nullable=False),
        pa.field("trade_id", pa.string(), nullable=False),
        pa.field("buyer", pa.string(), nullable=False),
        pa.field("seller", pa.string(), nullable=False),
        pa.field("hash", pa.string(), nullable=False),
        pa.field("ts_event", pa.uint64(), nullable=False),
        pa.field("ts_init", pa.uint64(), nullable=False),
    ],
    metadata={"type_name": "HyperliquidPublicTrade"},
)

register_arrow(
    HyperliquidPublicTrade,
    HYPERLIQUID_PUBLIC_TRADE_ARROW_SCHEMA,
    decoder=_decode_hyperliquid_public_trades,
)


__all__ = [
    "HYPERLIQUID",
    "HYPERLIQUID_CLIENT_ID",
    "HYPERLIQUID_VENUE",
    "HyperliquidAllDexsAssetCtxs",
    "HyperliquidAllMids",
    "HyperliquidDataClientConfig",
    "HyperliquidDexAssetCtx",
    "HyperliquidExecClientConfig",
    "HyperliquidImpactPrices",
    "HyperliquidInstrumentProvider",
    "HyperliquidLiveDataClientFactory",
    "HyperliquidLiveExecClientFactory",
    "HyperliquidOpenInterest",
    "HyperliquidProductType",
    "HyperliquidPublicTrade",
]
