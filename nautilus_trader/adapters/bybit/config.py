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

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.common.enums import BybitMarginMode
    from nautilus_trader.adapters.bybit.common.enums import BybitPositionMode
    from nautilus_trader.adapters.bybit.common.enums import BybitProductType
    from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol


class BybitDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BybitDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_KEY` or
        `BYBIT_TESTNET_API_KEY` environment variables.
    api_secret : str, optional
        The Bybit API public key.
        If ``None`` then will source the `BYBIT_API_SECRET` or
        `BYBIT_TESTNET_API_SECRET` environment variables.
    product_types : list[BybitProductType], optional
        The Bybit product type for the client.
        If not specified then will use all products.
    demo : bool, default False
        If the client is connecting to the Bybit demo API.
    testnet : bool, default False
        If the client is connecting to the Bybit testnet API.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    recv_window_ms : PositiveInt, default 5000
        The receive window (milliseconds) for Bybit HTTP requests.
    bars_timestamp_on_close : bool, default True
        If the ts_event timestamp for bars should be on the open or close or the bar.
        If True, then ts_event will be on the close of the bar.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: list[BybitProductType] | None = None
    base_url_http: str | None = None
    demo: bool = False
    testnet: bool = False
    update_instruments_interval_mins: PositiveInt | None = 60
    recv_window_ms: PositiveInt = 5_000
    bars_timestamp_on_close: bool = True


class BybitExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BybitExecutionClient`` instances.

    Parameters
    ----------
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
        If None then will default to 'SPOT', you also cannot mix 'SPOT' with
        any other product type for execution, and it will use a `CASH` account
        type, vs `MARGIN` for the other derivative products.
    base_url_ws_private : str, optional
        The base URL for the `private` WebSocket client.
    base_url_ws_trade : str, optional
        The base URL for the `trade` WebSocket client.
    demo : bool, default False
        If the client is connecting to the Bybit demo API.
    testnet : bool, default False
        If the client is connecting to the Bybit testnet API.
    use_gtd : bool, default False
        If False, then GTD time in force will be remapped to GTC
        (this is useful if managing GTD orders locally).
    use_ws_execution_fast : bool, default False
        If use fast execution stream.
    use_ws_trade_api : bool, default False
        If the client is using websocket to send order requests.
    use_http_batch_api : bool, default False
        If the client is using http api to send batch order requests.
        Effective only when `use_ws_trade_api` is set to `True`.
    use_spot_position_reports : bool, default False
        If True, wallet balances for SPOT instruments will be reported as positions:
        - Positive balances are reported as LONG positions.
        - Negative balances (borrowing) are reported as SHORT positions.
        - Zero balances (after rounding to instrument precision) are reported as FLAT.
        WARNING: This may lead to unintended liquidation of wallet assets if strategies
        are not designed to handle spot positions appropriately.
    ignore_uncached_instrument_executions : bool, default False
        If True, execution message for instruments not contained in the cache are ignored instead of raising an error.
    max_retries : PositiveInt, optional
        The maximum number of times a submit, cancel or modify order request will be retried.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries. Short delays with frequent retries may result in account bans.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.
    recv_window_ms : PositiveInt, default 5000
        The receive window (milliseconds) for Bybit HTTP requests.
    ws_trade_timeout_secs : PositiveFloat, default 5.0
        The timeout for trade websocket messages.
    ws_auth_timeout_secs : PositiveFloat, default 5.0
        The timeout for auth websocket messages.
    futures_leverages : dict[BybitSymbol, PositiveInt], optional
        The leverages for futures.
    position_mode : dict[BybitSymbol, BybitPositionMode], optional
        The position mode for `USDT perpetual` and `Inverse futures`.
    margin_mode : BybitMarginMode, optional
        Set Margin Mode.

    Warnings
    --------
    A short `retry_delay` with frequent retries may result in account bans.

    """

    api_key: str | None = None
    api_secret: str | None = None
    product_types: list[BybitProductType] | None = None
    base_url_http: str | None = None
    base_url_ws_private: str | None = None
    base_url_ws_trade: str | None = None
    demo: bool = False
    testnet: bool = False
    use_gtd: bool = False  # Not supported on Bybit
    use_ws_execution_fast: bool = False
    use_ws_trade_api: bool = False
    use_http_batch_api: bool = False
    ignore_uncached_instrument_executions: bool = False
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None
    recv_window_ms: PositiveInt = 5_000
    ws_trade_timeout_secs: PositiveFloat | None = 5.0
    ws_auth_timeout_secs: PositiveFloat | None = 5.0
    futures_leverages: dict[BybitSymbol, PositiveInt] | None = None
    position_mode: dict[BybitSymbol, BybitPositionMode] | None = None
    margin_mode: BybitMarginMode | None = None
    use_spot_position_reports: bool = False
