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
Instrument catalogue loading for the Binance adapter.
"""

import asyncio

from nautilus_trader._libnautilus.binance import BinanceDataClientConfig
from nautilus_trader._libnautilus.binance import _load_binance_instruments


async def load_binance_instruments(config: BinanceDataClientConfig) -> list[object]:
    """
    Load the configured Binance instrument catalogue.

    This is the Python v2 replacement for constructing a cached low-level HTTP client
    and a product-specific v1 instrument provider. The embedded ``instrument_provider``
    config controls selection, filters, parser warnings, and commission queries.

    """
    return await asyncio.to_thread(_load_binance_instruments, config)
