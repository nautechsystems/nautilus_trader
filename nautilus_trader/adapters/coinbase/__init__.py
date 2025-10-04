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
Coinbase Advanced Trade API adapter for NautilusTrader.

This adapter provides integration with Coinbase's Advanced Trade API for spot cryptocurrency trading.
"""

from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase.config import CoinbaseExecClientConfig
from nautilus_trader.adapters.coinbase.factories import CoinbaseLiveDataClientFactory
from nautilus_trader.adapters.coinbase.factories import CoinbaseLiveExecClientFactory


__all__ = [
    "CoinbaseDataClientConfig",
    "CoinbaseExecClientConfig",
    "CoinbaseLiveDataClientFactory",
    "CoinbaseLiveExecClientFactory",
]

