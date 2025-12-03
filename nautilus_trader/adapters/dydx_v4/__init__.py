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
DYdX v4 adapter (Rust-backed implementation).

This is a temporary namespace during the transition from dYdX v3 to v4.
Once the v4 adapter is fully stable, it will replace the legacy `dydx` adapter.

The v4 adapter uses Rust gRPC bindings for order execution, providing:
- Native Cosmos SDK transaction signing via Rust
- Direct validator node communication
- Improved performance and reliability

Usage:
    from nautilus_trader.adapters.dydx_v4 import DYDXv4ExecutionClient
    from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClient

"""

from nautilus_trader.adapters.dydx_v4.common.urls import get_grpc_url
from nautilus_trader.adapters.dydx_v4.common.urls import get_grpc_urls
from nautilus_trader.adapters.dydx_v4.common.urls import get_http_url
from nautilus_trader.adapters.dydx_v4.common.urls import get_ws_url


__all__ = [
    "get_grpc_url",
    "get_grpc_urls",
    "get_http_url",
    "get_ws_url",
]
