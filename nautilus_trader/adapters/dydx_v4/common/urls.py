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
URL helpers for dYdX v4.

These functions expose the Rust URL constants via PyO3 bindings.

"""

from nautilus_trader.core.nautilus_pyo3 import get_dydx_grpc_url  # type: ignore[attr-defined]
from nautilus_trader.core.nautilus_pyo3 import get_dydx_grpc_urls  # type: ignore[attr-defined]
from nautilus_trader.core.nautilus_pyo3 import get_dydx_http_url  # type: ignore[attr-defined]
from nautilus_trader.core.nautilus_pyo3 import get_dydx_ws_url  # type: ignore[attr-defined]


def get_grpc_urls(is_testnet: bool = False) -> list[str]:
    """
    Get the gRPC URLs for dYdX v4 with fallback support.

    Parameters
    ----------
    is_testnet : bool, default False
        Whether to use testnet URLs.

    Returns
    -------
    list[str]
        List of gRPC URLs to try in order.

    """
    return get_dydx_grpc_urls(is_testnet)


def get_grpc_url(is_testnet: bool = False) -> str:
    """
    Get the primary gRPC URL for dYdX v4.

    Parameters
    ----------
    is_testnet : bool, default False
        Whether to use testnet URL.

    Returns
    -------
    str
        The primary gRPC URL.

    """
    return get_dydx_grpc_url(is_testnet)


def get_http_url(is_testnet: bool = False) -> str:
    """
    Get the HTTP base URL for dYdX v4 Indexer API.

    Parameters
    ----------
    is_testnet : bool, default False
        Whether to use testnet URL.

    Returns
    -------
    str
        The HTTP base URL.

    """
    return get_dydx_http_url(is_testnet)


def get_ws_url(is_testnet: bool = False) -> str:
    """
    Get the WebSocket URL for dYdX v4 Indexer API.

    Parameters
    ----------
    is_testnet : bool, default False
        Whether to use testnet URL.

    Returns
    -------
    str
        The WebSocket URL.

    """
    return get_dydx_ws_url(is_testnet)
