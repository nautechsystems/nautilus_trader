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
DYdX v4 cryptocurrency exchange adapter (Rust-backed implementation).

This is a temporary namespace during the transition from dYdX v3 to v4.
Once the v4 adapter is fully stable, it will replace the legacy `dydx` adapter.

The v4 adapter uses Rust-backed HTTP, WebSocket, and gRPC clients for:
- Native Cosmos SDK transaction signing via Rust
- Direct validator node communication
- Improved performance and reliability
- Real-time market data streaming

Usage:
    from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClientConfig
    from nautilus_trader.adapters.dydx_v4 import DYDXv4ExecClientConfig
    from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveDataClientFactory
    from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveExecClientFactory

"""

from nautilus_trader.adapters.dydx_v4.common.urls import get_grpc_url
from nautilus_trader.adapters.dydx_v4.common.urls import get_grpc_urls
from nautilus_trader.adapters.dydx_v4.common.urls import get_http_url
from nautilus_trader.adapters.dydx_v4.common.urls import get_ws_url
from nautilus_trader.adapters.dydx_v4.config import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4.config import DYDXv4ExecClientConfig
from nautilus_trader.adapters.dydx_v4.constants import DYDX
from nautilus_trader.adapters.dydx_v4.constants import DYDX_CLIENT_ID
from nautilus_trader.adapters.dydx_v4.constants import DYDX_VENUE
from nautilus_trader.adapters.dydx_v4.data import DYDXv4DataClient
from nautilus_trader.adapters.dydx_v4.execution import DYDXv4ExecutionClient
from nautilus_trader.adapters.dydx_v4.factories import DYDXv4LiveDataClientFactory
from nautilus_trader.adapters.dydx_v4.factories import DYDXv4LiveExecClientFactory
from nautilus_trader.adapters.dydx_v4.providers import DYDXv4InstrumentProvider
from nautilus_trader.adapters.dydx_v4.types import DYDX_INSTRUMENT_TYPES
from nautilus_trader.adapters.dydx_v4.types import DydxInstrument


__all__ = [
    "DYDX",
    "DYDX_CLIENT_ID",
    "DYDX_INSTRUMENT_TYPES",
    "DYDX_VENUE",
    "DYDXv4DataClient",
    "DYDXv4DataClientConfig",
    "DYDXv4ExecClientConfig",
    "DYDXv4ExecutionClient",
    "DYDXv4InstrumentProvider",
    "DYDXv4LiveDataClientFactory",
    "DYDXv4LiveExecClientFactory",
    "DydxInstrument",
    "get_grpc_url",
    "get_grpc_urls",
    "get_http_url",
    "get_ws_url",
]
