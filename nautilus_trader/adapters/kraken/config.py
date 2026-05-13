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

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.model.enums import AccountType


class KrakenDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``KrakenDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API public key.
        If ``None`` then will source from environment variables:
        - Spot: `KRAKEN_SPOT_API_KEY`
        - Futures: `KRAKEN_FUTURES_API_KEY` or `KRAKEN_FUTURES_DEMO_API_KEY`
    api_secret : str, optional
        The Kraken API secret key.
        If ``None`` then will source from environment variables:
        - Spot: `KRAKEN_SPOT_API_SECRET`
        - Futures: `KRAKEN_FUTURES_API_SECRET` or `KRAKEN_FUTURES_DEMO_API_SECRET`
    environment : KrakenEnvironment, optional
        The Kraken environment to connect to.
        If ``None`` then defaults to ``KrakenEnvironment.LIVE``.
        Note: demo is only available for Futures.
    product_types : tuple[KrakenProductType, ...], optional
        The Kraken product types for the client.
        If ``None`` then defaults to ``(KrakenProductType.SPOT,)``.
    base_url_http_spot : str, optional
        The base URL for Kraken Spot HTTP API.
        If ``None`` then will use the default URL based on environment.
    base_url_http_futures : str, optional
        The base URL for Kraken Futures HTTP API.
        If ``None`` then will use the default URL based on environment.
    base_url_ws_spot : str, optional
        The base URL for Kraken Spot WebSocket API.
        If ``None`` then will use the default URL based on environment.
    base_url_ws_futures : str, optional
        The base URL for Kraken Futures WebSocket API.
        If ``None`` then will use the default URL based on environment.
    base_url_ws_l3_spot : str, optional
        The base URL for Kraken Spot L3 WebSocket API.
        If ``None`` then will use the default URL based on environment.
    proxy_url : str, optional
        Optional proxy URL for HTTP and WebSocket transports.
    update_instruments_interval_mins: PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.
    max_retries : PositiveInt, optional
        The maximum number of times an HTTP request will be retried.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.
    http_timeout_secs : PositiveInt, optional
        The timeout in seconds for HTTP requests.
    ws_heartbeat_secs : PositiveInt, default 30
        The WebSocket heartbeat interval in seconds.
    max_requests_per_second : PositiveInt, optional
        The maximum number of requests per second for rate limiting.
        If ``None`` then will use the default of 5 requests per second.
    validate_l3_checksum : bool, default True
        If True, CRC32 checksums on ``level3`` book updates are verified and
        a clear delta is emitted downstream on mismatch.

    """

    api_key: str | None = None
    api_secret: str | None = None
    environment: KrakenEnvironment | None = None
    product_types: tuple[KrakenProductType, ...] | None = None
    base_url_http_spot: str | None = None
    base_url_http_futures: str | None = None
    base_url_ws_spot: str | None = None
    base_url_ws_futures: str | None = None
    base_url_ws_l3_spot: str | None = None
    proxy_url: str | None = None
    update_instruments_interval_mins: PositiveInt | None = 60
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None
    http_timeout_secs: PositiveInt | None = None
    ws_heartbeat_secs: PositiveInt = 30
    max_requests_per_second: PositiveInt | None = None
    validate_l3_checksum: bool = True


class KrakenExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``KrakenExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API public key.
        If ``None`` then will source from environment variables:
        - Spot: `KRAKEN_SPOT_API_KEY`
        - Futures: `KRAKEN_FUTURES_API_KEY` or `KRAKEN_FUTURES_DEMO_API_KEY`
    api_secret : str, optional
        The Kraken API secret key.
        If ``None`` then will source from environment variables:
        - Spot: `KRAKEN_SPOT_API_SECRET`
        - Futures: `KRAKEN_FUTURES_API_SECRET` or `KRAKEN_FUTURES_DEMO_API_SECRET`
    environment : KrakenEnvironment, optional
        The Kraken environment to connect to.
        If ``None`` then defaults to ``KrakenEnvironment.LIVE``.
        Note: demo is only available for Futures.
    product_types : tuple[KrakenProductType, ...], optional
        The Kraken product types for the client.
        If ``None`` then defaults to ``(KrakenProductType.SPOT,)``.
        Note: FUTURES always uses MARGIN; SPOT uses ``spot_account_type`` (default CASH).
    base_url_http_spot : str, optional
        The base URL for Kraken Spot HTTP API.
        If ``None`` then will use the default URL based on environment.
    base_url_http_futures : str, optional
        The base URL for Kraken Futures HTTP API.
        If ``None`` then will use the default URL based on environment.
    base_url_ws_spot : str, optional
        The base URL for Kraken Spot WebSocket API.
        If ``None`` then will use the default URL based on environment.
    base_url_ws_futures : str, optional
        The base URL for Kraken Futures WebSocket API.
        If ``None`` then will use the default URL based on environment.
    proxy_url : str, optional
        Optional proxy URL for HTTP and WebSocket transports.
    max_retries : PositiveInt, optional
        The maximum number of times an HTTP request will be retried.
    retry_delay_initial_ms : PositiveInt, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, optional
        The maximum delay (milliseconds) between retries.
    http_timeout_secs : PositiveInt, optional
        The timeout in seconds for HTTP requests.
    ws_heartbeat_secs : PositiveInt, default 30
        The WebSocket heartbeat interval in seconds.
    max_requests_per_second : PositiveInt, optional
        The maximum number of requests per second for rate limiting.
        If ``None`` then will use the default of 5 requests per second.
    use_spot_position_reports : bool, default False
        If True, wallet balances for SPOT instruments will be reported as positions:
        - Positive balances are reported as LONG positions.
        - Zero balances (after rounding to instrument precision) are reported as FLAT.
        WARNING: This may lead to unintended liquidation of wallet assets if strategies
        are not designed to handle spot positions appropriately.
    spot_positions_quote_currency : str, default "USDT"
        The quote currency to use when generating spot position reports.
        Only instruments with this quote currency will have positions reported.
    spot_account_type : AccountType, default AccountType.CASH
        The account type for spot trading. Set to ``AccountType.MARGIN`` to enable:
        - ``TradeBalance``-based margin reporting (used margin, free margin, equity).
        - ``OpenPositions``-based position reconciliation.
        - Per-order leverage via ``SubmitOrder.params = {"leverage": N}``.
        Has no effect when ``product_types`` includes ``KrakenProductType.FUTURES``.
    default_leverage : int, optional
        Default leverage multiplier for spot margin orders when not specified per-order.
        For example, ``3`` sends ``"3:1"`` to Kraken. ``None`` means cash (no leverage sent).
        Per-order override: pass ``{"leverage": N}`` in ``SubmitOrder.params``.
    margin_balance_asset : str, optional
        Summary-display asset for ``TradeBalance`` margin metrics (e.g. ``"ZUSD"``,
        ``"ZGBP"``, ``"ZEUR"``, ``"USDT"``). Controls the denomination of equity,
        free margin, used margin, and other summary figures returned by Kraken's
        ``TradeBalance`` endpoint. ``None`` lets Kraken default to ``ZUSD``.
        Display-only: Kraken converts internally; per-position figures from
        ``OpenPositions`` remain in the traded pair's quote currency.
        Only effective when ``spot_account_type=AccountType.MARGIN``.

    Examples
    --------
    Spot margin account with 3x default leverage:

    .. code-block:: python

        from nautilus_trader.model.enums import AccountType
        from nautilus_trader.adapters.kraken.config import KrakenExecClientConfig

        config = KrakenExecClientConfig(
            api_key="...",
            api_secret="...",
            spot_account_type=AccountType.MARGIN,
            default_leverage=3,
        )

    Override leverage on a single order:

    .. code-block:: python

        order = strategy.order_factory.limit(
            instrument_id=BTC_USD,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.01"),
            price=Price.from_str("50000.00"),
            params={"leverage": 5},  # overrides default_leverage for this order
        )
        strategy.submit_order(order)

    """

    api_key: str | None = None
    api_secret: str | None = None
    environment: KrakenEnvironment | None = None
    product_types: tuple[KrakenProductType, ...] | None = None
    base_url_http_spot: str | None = None
    base_url_http_futures: str | None = None
    base_url_ws_spot: str | None = None
    base_url_ws_futures: str | None = None
    proxy_url: str | None = None
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None
    http_timeout_secs: PositiveInt | None = None
    ws_heartbeat_secs: PositiveInt = 30
    max_requests_per_second: PositiveInt | None = None
    use_spot_position_reports: bool = False
    spot_positions_quote_currency: str = "USDT"
    spot_account_type: AccountType = AccountType.CASH
    default_leverage: int | None = None
    margin_balance_asset: str | None = None

    def __post_init__(self) -> None:
        if self.default_leverage is not None and self.spot_account_type != AccountType.MARGIN:
            raise ValueError(
                "default_leverage requires spot_account_type=AccountType.MARGIN "
                f"(got {self.spot_account_type!r})",
            )
