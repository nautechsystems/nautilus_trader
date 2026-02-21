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
DYdX v4 cryptocurrency exchange adapter (Rust-backed implementation).

The v4 adapter uses Rust-backed HTTP, WebSocket, and gRPC clients for:
- Native Cosmos SDK transaction signing via Rust
- Direct validator node communication
- Improved performance and reliability
- Real-time market data streaming

Usage:
    from nautilus_trader.adapters.dydx import DydxDataClientConfig
    from nautilus_trader.adapters.dydx import DydxExecClientConfig
    from nautilus_trader.adapters.dydx import DydxLiveDataClientFactory
    from nautilus_trader.adapters.dydx import DydxLiveExecClientFactory

"""

from nautilus_trader.adapters.dydx.config import DydxDataClientConfig
from nautilus_trader.adapters.dydx.config import DydxExecClientConfig
from nautilus_trader.adapters.dydx.constants import DYDX
from nautilus_trader.adapters.dydx.constants import DYDX_CLIENT_ID
from nautilus_trader.adapters.dydx.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx.data import DydxDataClient
from nautilus_trader.adapters.dydx.execution import DydxExecutionClient
from nautilus_trader.adapters.dydx.factories import DydxLiveDataClientFactory
from nautilus_trader.adapters.dydx.factories import DydxLiveExecClientFactory
from nautilus_trader.adapters.dydx.providers import DydxInstrumentProvider


__all__ = [
    "DYDX",
    "DYDX_CLIENT_ID",
    "DYDX_VENUE",
    "DydxDataClient",
    "DydxDataClientConfig",
    "DydxExecClientConfig",
    "DydxExecutionClient",
    "DydxInstrumentProvider",
    "DydxLiveDataClientFactory",
    "DydxLiveExecClientFactory",
]
