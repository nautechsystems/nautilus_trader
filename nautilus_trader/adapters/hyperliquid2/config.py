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
Configuration classes for the Hyperliquid adapter.
"""

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class Hyperliquid2DataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``Hyperliquid2LiveDataClient`` instances.

    Parameters
    ----------
    private_key : str | None, optional
        The Ethereum private key for authentication (hex string).
        If ``None``, will try to load from environment variable HYPERLIQUID_PRIVATE_KEY.
    http_base : str | None, optional
        The base HTTP URL for the Hyperliquid API.
        If ``None``, defaults to https://api.hyperliquid.xyz.
    ws_base : str | None, optional
        The base WebSocket URL for Hyperliquid streams.
        If ``None``, defaults to wss://api.hyperliquid.xyz/ws.
    testnet : bool, default False
        If True, connect to Hyperliquid testnet instead of mainnet.
    """

    private_key: str | None = None
    http_base: str | None = None
    ws_base: str | None = None
    testnet: bool = False


class Hyperliquid2ExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``Hyperliquid2LiveExecClient`` instances.

    Parameters
    ----------
    private_key : str | None, optional
        The Ethereum private key for authentication (hex string).
        If ``None``, will try to load from environment variable HYPERLIQUID_PRIVATE_KEY.
    http_base : str | None, optional
        The base HTTP URL for the Hyperliquid API.
        If ``None``, defaults to https://api.hyperliquid.xyz.
    ws_base : str | None, optional
        The base WebSocket URL for Hyperliquid streams.
        If ``None``, defaults to wss://api.hyperliquid.xyz/ws.
    testnet : bool, default False
        If True, connect to Hyperliquid testnet instead of mainnet.
    """

    private_key: str | None = None
    http_base: str | None = None
    ws_base: str | None = None
    testnet: bool = False
