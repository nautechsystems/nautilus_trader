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

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceKeyType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.identifiers import Venue


class BinanceDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BinanceDataClient`` instances.

    Parameters
    ----------
    venue : Venue, default BINANCE_VENUE
        The venue for the client.
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_SECRET` or
        `BINANCE_TESTNET_API_SECRET` environment variables.
    key_type : BinanceKeyType, default 'HMAC'
        The private key cryptographic algorithm type.
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
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    use_agg_trade_ticks : bool, default False
        Whether to use aggregated trade tick endpoints instead of raw trades.
        TradeId of ticks will be the Aggregate tradeId returned by Binance.

    """

    venue: Venue = BINANCE_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    key_type: BinanceKeyType = BinanceKeyType.HMAC
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: str | None = None
    base_url_ws: str | None = None
    us: bool = False
    testnet: bool = False
    update_instruments_interval_mins: PositiveInt | None = 60
    use_agg_trade_ticks: bool = False


class BinanceExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BinanceExecutionClient`` instances.

    Parameters
    ----------
    venue : Venue, default BINANCE_VENUE
        The venue for the client.
    api_key : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Binance API public key.
        If ``None`` then will source the `BINANCE_API_KEY` or
        `BINANCE_TESTNET_API_KEY` environment variables.
    key_type : BinanceKeyType, default 'HMAC'
        The private key cryptographic algorithm type.
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
        If False, then GTD time in force will be remapped to GTC (this is useful if managing GTD orders locally).
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders is sent through to the exchange.
        If True, then will assign the value on orders sent to the exchange, otherwise will always be False.
    use_position_ids: bool, default True
        If Binance Futures hedging position IDs should be used.
        If False, then order event `position_id`(s) from the execution client will be `None`, which
        allows *virtual* positions with `OmsType.HEDGING`.
    use_trade_lite: bool, default False
        If TRADE_LITE events should be used.
        If True, commissions will be calculated based on the instrument's details.
    treat_expired_as_canceled : bool, default False
        If the `EXPIRED` execution type is semantically treated as `CANCELED`.
        Binance treats cancels with certain combinations of order type and time in force as expired
        events. This config option allows you to treat these uniformally as cancels.
    recv_window_ms : PositiveInt, default 5000
        The receive window (milliseconds) for Binance HTTP requests.
    max_retries : PositiveInt, optional
        The maximum number of times a submit, cancel or modify order request will be retried.
    retry_delay : PositiveFloat, optional
        The delay (seconds) between retries. Short delays with frequent retries may result in account bans.
    futures_leverages : dict[BinanceSymbol, PositiveInt], optional
        The initial leverage to be used for each symbol. It's applicable to futures only.

    Warnings
    --------
    A short `retry_delay` with frequent retries may result in account bans.

    """

    venue: Venue = BINANCE_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    key_type: BinanceKeyType = BinanceKeyType.HMAC
    account_type: BinanceAccountType = BinanceAccountType.SPOT
    base_url_http: str | None = None
    base_url_ws: str | None = None
    us: bool = False
    testnet: bool = False
    use_gtd: bool = True
    use_reduce_only: bool = True
    use_position_ids: bool = True
    use_trade_lite: bool = False
    treat_expired_as_canceled: bool = False
    recv_window_ms: PositiveInt = 5_000
    max_retries: PositiveInt | None = None
    retry_delay: PositiveFloat | None = None
    futures_leverages: dict[BinanceSymbol, PositiveInt] | None = None
