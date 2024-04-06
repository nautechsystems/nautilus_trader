# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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


from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt


class BybitDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BybitDataClient`` instances.

    api_key : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_KEY` or
        `BYBIT_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_KEY` or
        `BYBIT_TESTNET_API_KEY` environment variables.
    product_types : list[BybitProductType], optional
        The Bybit product type for the client.
        If not specified then will use all products.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: list[BybitProductType] | None = None
    http_base_url: str | None = None
    testnet: bool = False


class BybitExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BybitExecutionClient`` instances.

    api_key : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_KEY` or
        `BYBIT_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_KEY` or
        `BYBIT_TESTNET_API_KEY` environment variables.
    product_type : list[BybitProductType], optional
        The Bybit product type for the client.
        If None then will default to 'SPOT', you also cannot mix 'SPOT' with
        any other product type for execution, and it will use a `CASH` account
        type, vs `MARGIN` for the other derivative products.
    use_gtd : bool, default False
        If False then GTD time in force will be remapped to GTC
        (this is useful if managing GTD orders locally).

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: list[BybitProductType] | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False
    use_gtd: bool = False  # Not supported on Bybit
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
