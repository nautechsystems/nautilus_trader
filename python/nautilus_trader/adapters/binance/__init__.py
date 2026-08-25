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
Integration adapter for the Binance exchange.
"""

from __future__ import annotations

from nautilus_trader._fixup import fixup_module_names
from nautilus_trader._libnautilus.binance import *  # noqa: F403 (undefined-local-with-import-star)
from nautilus_trader.adapters.binance.instruments import (
    load_binance_instruments as load_binance_instruments,
)


__all__ = [
    "BINANCE",
    "BINANCE_CLIENT_ID",
    "BINANCE_VENUE",
    "BinanceBar",
    "BinanceDataClientConfig",
    "BinanceDataClientFactory",
    "BinanceEnvironment",
    "BinanceExecutionClientConfig",
    "BinanceExecutionClientFactory",
    "BinanceFuturesLiquidation",
    "BinanceFuturesMarkPriceUpdate",
    "BinanceFuturesOpenInterest",
    "BinanceFuturesOpenInterestHist",
    "BinanceFuturesOpenInterestHistPoint",
    "BinanceFuturesTicker",
    "BinanceInstrumentProviderConfig",
    "BinanceMarginType",
    "BinancePositionSide",
    "BinanceProductType",
    "BinanceSpotMarketDataMode",
    "BinanceSpotTicker",
    "decode_binance_futures_client_order_id",
    "decode_binance_spot_client_order_id",
    "get_binance_arrow_schema_map",
    "load_binance_instruments",
    "load_binance_order_book_deltas",
]

fixup_module_names(globals(), __name__)
del fixup_module_names
