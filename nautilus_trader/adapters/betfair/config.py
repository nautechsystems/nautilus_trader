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

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.common.config import NonNegativeInt
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class BetfairDataClientConfig(LiveDataClientConfig, kw_only=True, frozen=True):
    """
    Configuration for ``BetfairDataClient`` instances.

    Parameters
    ----------
    account_currency : str
        The currency for the Betfair account.
    username : str, optional
        The Betfair account username.
    password : str, optional
        The Betfair account password.
    app_key : str, optional
        The Betfair application key.
    certs_dir : str, optional
        The local directory that contains the Betfair SSL certificates.
    instrument_config : BetfairInstrumentProviderConfig, None
        The Betfair instrument provider config.
    subscription_delay_secs : PositiveInt, default 3
        The delay (seconds) before sending the *initial* subscription message.
    keep_alive_secs : PositiveInt, default 36_000 (10 hours)
        The keep alive interval (seconds) for the HTTP client.
    subscribe_race_data : bool, default False
        If True, sends a ``raceSubscription`` on the stream to receive Race Change
        Messages (RCM) with live GPS tracking data (Total Performance Data).
    stream_conflate_ms : PositiveInt, optional
        The Betfair data stream conflation setting. Default of `None` means no explicit value is
        set for the conflation interval. Betfair interprets this as using its default behaviour for
        conflation. The default typically applies conflation, so you need to ensure
        stream_conflate_ms=0 is explicitly set to guarantee no conflation.
    stream_heartbeat_ms : PositiveInt, default 5000
        The Betfair stream heartbeat interval (milliseconds). Betfair will send
        heartbeat messages at this interval during quiet periods, preventing
        silent connection drops. Valid range is 500-5000. Set to `None` to omit.
    proxy_url : str, optional
        The proxy URL for HTTP requests.

    """

    account_currency: str
    username: str | None = None
    password: str | None = None
    app_key: str | None = None
    certs_dir: str | None = None
    instrument_config: BetfairInstrumentProviderConfig | None = None
    subscription_delay_secs: PositiveInt | None = 3
    keep_alive_secs: PositiveInt = 36_000  # 10 hours
    subscribe_race_data: bool = False
    stream_conflate_ms: PositiveInt | None = None
    stream_heartbeat_ms: PositiveInt | None = 5000
    proxy_url: str | None = None


class BetfairExecClientConfig(LiveExecClientConfig, kw_only=True, frozen=True):
    """
    Configuration for ``BetfairExecClient`` instances.

    Parameters
    ----------
    account_currency : str
        The currency for the Betfair account.
    username : str, optional
        The Betfair account username.
    password : str, optional
        The Betfair account password.
    app_key : str, optional
        The Betfair application key.
    certs_dir : str, optional
        The local directory that contains the Betfair SSL certificates.
    instrument_config : BetfairInstrumentProviderConfig, None
        The Betfair instrument provider config.
    calculate_account_state : bool, default True
        If the Betfair account state should be calculated from events.
    request_account_state_secs : NonNegativeInt, default 300 (5 minutes)
        The request interval (seconds) for account state checks.
        If zero, then will not request account state from Betfair.
    reconcile_market_ids_only : bool, default False
        If True, reconciliation only requests orders matching the market IDs listed
        in the `instrument_config`. If False, all orders are reconciled.
    stream_market_ids_filter : list[str], optional
        If provided, only process order stream updates for these market IDs.
        Updates for other markets are silently skipped. Useful to reduce warning
        spam when sharing a Betfair account across multiple trading nodes.
    ignore_external_orders : bool, default False
        If True, orders received over the stream that aren't found in the cache
        will be silently ignored. This is useful when multiple trading nodes
        share the same Betfair account across different markets.
    use_market_version : bool, default False
        If True, automatically attach the latest market version to placeOrders
        and replaceOrders requests. When the market version has advanced beyond
        the version sent with the order, Betfair will lapse the bet rather than
        matching it against a changed book. This provides price protection by
        preventing orders from being matched when the market has moved.
    order_request_rate_per_second : PositiveInt, default 20
        The rate limit (requests/second) for order endpoints (placeOrders,
        replaceOrders, cancelOrders). Order endpoints use a separate rate
        limit bucket from general API endpoints (5/sec) so order placement
        is not throttled by account state polling or reconciliation queries.
    stream_heartbeat_ms : PositiveInt, default 5000
        The Betfair order stream heartbeat interval (milliseconds). Betfair will
        send heartbeat messages at this interval during quiet periods, preventing
        silent connection drops. Valid range is 500-5000. Set to `None` to omit.
    proxy_url : str, optional
        The proxy URL for HTTP requests.

    """

    account_currency: str
    username: str | None = None
    password: str | None = None
    app_key: str | None = None
    certs_dir: str | None = None
    instrument_config: BetfairInstrumentProviderConfig | None = None
    calculate_account_state: bool = True
    request_account_state_secs: NonNegativeInt = 300
    reconcile_market_ids_only: bool = False
    stream_market_ids_filter: list[str] | None = None
    ignore_external_orders: bool = False
    use_market_version: bool = False
    order_request_rate_per_second: PositiveInt = 20
    stream_heartbeat_ms: PositiveInt | None = 5000
    proxy_url: str | None = None
