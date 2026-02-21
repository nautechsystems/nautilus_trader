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
Configuration classes for dYdX v4 adapter.

These classes provide Python-side configuration for the Rust-backed dYdX v4 clients.

"""

from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class DydxDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``DydxDataClient`` instances.

    Parameters
    ----------
    wallet_address : str, optional
        The dYdX wallet address.
        If ``None`` then will source `DYDX_WALLET_ADDRESS` or
        `DYDX_TESTNET_WALLET_ADDRESS` environment variables.
    is_testnet : bool, default False
        If the client is connecting to the dYdX testnet API.
    base_url_http : str, optional
        The base URL for HTTP API endpoints.
        If ``None`` then will use the default URL for the selected network.
    base_url_ws : str, optional
        The base URL for WebSocket connections.
        If ``None`` then will use the default URL for the selected network.
    bars_timestamp_on_close : bool, default True
        If bar `ts_event` timestamps should be the bar close time.
        If False, the venue-native open time will be used.
    max_retries : PositiveInt, optional
        The maximum number of retries for HTTP requests or websocket reconnects.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.

    """

    wallet_address: str | None = None
    is_testnet: bool = False
    bars_timestamp_on_close: bool = True
    base_url_http: str | None = None
    base_url_ws: str | None = None
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000


class DydxExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``DydxExecutionClient`` instances.

    Parameters
    ----------
    wallet_address : str, optional
        The dYdX wallet address.
        If ``None`` then will source `DYDX_WALLET_ADDRESS` or
        `DYDX_TESTNET_WALLET_ADDRESS` environment variables.
    subaccount : int, default 0
        The subaccount number.
        The venue creates subaccount 0 by default.
    private_key : str, optional
        The hex-encoded private key used to sign transactions like submitting orders.
        If ``None`` then will source `DYDX_PRIVATE_KEY` or
        `DYDX_TESTNET_PRIVATE_KEY` environment variables.
    authenticator_ids : list[int], optional
        List of authenticator IDs for permissioned key trading.
        When provided, transactions will include a TxExtension to enable trading
        via sub-accounts using delegated signing keys. This is an advanced feature
        for institutional setups with separated hot/cold wallet architectures.
    is_testnet : bool, default False
        If the client is connecting to the dYdX testnet API.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
        If ``None`` then will use the default URL for the selected network.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
        If ``None`` then will use the default URL for the selected network.
    base_url_grpc : str, optional
        The gRPC client custom endpoint override.
        If ``None`` then will use the default URL for the selected network.
    max_retries : PositiveInt, optional
        The maximum number of times a submit, cancel or modify order request will be retried.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.
    grpc_rate_limit_per_second : PositiveInt, optional
        The maximum number of gRPC requests per second.
        Default ``4`` is safe for all known providers (Polkachu 5/s, KingNodes ~4.2/s, AutoStake 4/s).
        Set to ``None`` to disable rate limiting.

    """

    wallet_address: str | None = None
    subaccount: int = 0
    private_key: str | None = None
    authenticator_ids: list[int] | None = None
    is_testnet: bool = False
    base_url_http: str | None = None
    base_url_ws: str | None = None
    base_url_grpc: str | None = None
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    grpc_rate_limit_per_second: PositiveInt | None = 4
