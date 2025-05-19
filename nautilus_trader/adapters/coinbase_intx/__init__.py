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
Coinbase International crypto exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, data types and constants for connecting to and interacting with
Coinbase International's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.coinbase_intx``.

"""
from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxExecClientConfig
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_CLIENT_ID
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX_VENUE
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveDataClientFactory
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveExecClientFactory
from nautilus_trader.adapters.coinbase_intx.factories import get_coinbase_intx_http_client
from nautilus_trader.adapters.coinbase_intx.factories import get_coinbase_intx_instrument_provider
from nautilus_trader.adapters.coinbase_intx.providers import CoinbaseIntxInstrumentProvider


__all__ = [
    "COINBASE_INTX",
    "COINBASE_INTX_CLIENT_ID",
    "COINBASE_INTX_VENUE",
    "CoinbaseIntxDataClientConfig",
    "CoinbaseIntxExecClientConfig",
    "CoinbaseIntxInstrumentProvider",
    "CoinbaseIntxLiveDataClientFactory",
    "CoinbaseIntxLiveExecClientFactory",
    "get_coinbase_intx_http_client",
    "get_coinbase_intx_instrument_provider",
]
