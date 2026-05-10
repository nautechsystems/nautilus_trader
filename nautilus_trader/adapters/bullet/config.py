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

from __future__ import annotations

from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core.nautilus_pyo3 import BulletEnvironment


class BulletDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BulletDataClient`` instances.

    Parameters
    ----------
    environment : BulletEnvironment, default BulletEnvironment.Mainnet
        The Bullet environment (Mainnet, Testnet, or Staging).
    base_url_http : str, optional
        Override for the HTTP base URL.
    base_url_ws : str, optional
        Override for the WebSocket URL.
    proxy_url : str, optional
        Optional proxy URL for HTTP and WebSocket transports.
    http_timeout_secs : PositiveInt, default 60
        The timeout (seconds) for HTTP requests.
    update_instruments_interval_mins : PositiveInt, default 60
        Interval for refreshing instrument definitions.

    """

    environment: BulletEnvironment = BulletEnvironment.Mainnet
    base_url_http: str | None = None
    base_url_ws: str | None = None
    proxy_url: str | None = None
    http_timeout_secs: PositiveInt = 60
    update_instruments_interval_mins: PositiveInt = 60


class BulletExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BulletExecutionClient`` instances.

    Parameters
    ----------
    private_key : str, optional
        The Ed25519 delegate private key as a hex string.
        Falls back to the ``BULLET_PRIVATE_KEY`` environment variable.
    key_file : str, optional
        Path to a Solana-compatible JSON keystore file.
        Falls back to the ``BULLET_KEY_FILE`` environment variable.
    account_address : str, optional
        The main account address (base58-encoded public key).
        When absent the address is derived from the signing key (no-delegate mode).
        Falls back to the ``BULLET_ACCOUNT_ADDRESS`` environment variable.
    environment : BulletEnvironment, default BulletEnvironment.Mainnet
        The Bullet environment (Mainnet, Testnet, or Staging).
    base_url_http : str, optional
        Override for the HTTP base URL.
    base_url_ws : str, optional
        Override for the WebSocket URL.
    proxy_url : str, optional
        Optional proxy URL for HTTP and WebSocket transports.
    http_timeout_secs : PositiveInt, default 60
        The timeout (seconds) for HTTP requests.
    max_retries : PositiveInt, default 3
        Maximum number of retry attempts for failed requests.
    retry_delay_initial_ms : PositiveInt, default 1000
        Initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, default 10000
        Maximum delay (milliseconds) between retries.

    """

    private_key: str | None = None
    key_file: str | None = None
    account_address: str | None = None
    environment: BulletEnvironment = BulletEnvironment.Mainnet
    base_url_http: str | None = None
    base_url_ws: str | None = None
    proxy_url: str | None = None
    http_timeout_secs: PositiveInt = 60
    max_retries: PositiveInt = 3
    retry_delay_initial_ms: PositiveInt = 1000
    retry_delay_max_ms: PositiveInt = 10_000
