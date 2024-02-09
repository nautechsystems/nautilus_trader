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


from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt


class BinanceDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BinanceDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    account_type : BinanceAccountType, default BinanceAccountType.SPOT
        The account type for the client.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    us : bool, default False
        If client is connecting to Binance US.
    testnet : bool, default False
        If the client is connecting to a Binance testnet.
    use_agg_trade_ticks : bool, default False
        Whether to use aggregated trade tick endpoints instead of raw trade ticks.
        TradeId of ticks will be the Aggregate tradeId returned by Binance.

    """

    api_key: str | None = None
    api_secret: str | None = None
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: str | None = None
    base_url_ws: str | None = None
    us: bool = False
    testnet: bool = False
    use_agg_trade_ticks: bool = False


class BinanceExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BinanceExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    account_type : BinanceAccountType, default BinanceAccountType.SPOT
        The account type for the client.
    base_url_http : str, optional
        The HTTP client custom endpoint override.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
    us : bool, default False
        If client is connecting to Binance US.
    testnet : bool, default False
        If the client is connecting to a Binance testnet.
    use_gtd : bool, default True
        If GTD orders will use the Binance GTD TIF option.
        If False then GTD time in force will be remapped to GTC (this is useful if manageing GTD
        orders locally).
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders is sent through to the exchange.
        If True then will assign the value on orders sent to the exchange, otherwise will always be False.
    use_position_ids: bool, default True
        If Binance Futures hedging position IDs should be used.
        If False then order event `position_id`(s) from the execution client will be `None`, which
        allows *virtual* positions with `OmsType.HEDGING`.
    treat_expired_as_canceled : bool, default False
        If the `EXPIRED` execution type is semantically treated as `CANCELED`.
        Binance treats cancels with certain combinations of order type and time in force as expired
        events. This config option allows you to treat these uniformally as cancels.
    max_retries : PositiveInt, optional
        The maximum number of times a submit or cancel order request will be retried.
    retry_delay : PositiveFloat, optional
        The delay (seconds) between retries.

    """

    api_key: str | None = None
    api_secret: str | None = None
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: str | None = None
    base_url_ws: str | None = None
    us: bool = False
    testnet: bool = False
    clock_sync_interval_secs: int = 0
    use_gtd: bool = True
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
