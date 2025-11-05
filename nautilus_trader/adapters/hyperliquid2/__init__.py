# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
Hyperliquid adapter for Nautilus Trader.

This adapter provides connectivity to the Hyperliquid cryptocurrency exchange,
supporting perpetual futures trading with real-time market data and order execution.
"""

from nautilus_trader.adapters.hyperliquid2.config import Hyperliquid2DataClientConfig
from nautilus_trader.adapters.hyperliquid2.config import Hyperliquid2ExecClientConfig
from nautilus_trader.adapters.hyperliquid2.factories import Hyperliquid2LiveDataClientFactory
from nautilus_trader.adapters.hyperliquid2.factories import Hyperliquid2LiveExecClientFactory
from nautilus_trader.adapters.hyperliquid2.providers import Hyperliquid2InstrumentProvider


__all__ = [
    "Hyperliquid2DataClientConfig",
    "Hyperliquid2ExecClientConfig",
    "Hyperliquid2LiveDataClientFactory",
    "Hyperliquid2LiveExecClientFactory",
    "Hyperliquid2InstrumentProvider",
]
