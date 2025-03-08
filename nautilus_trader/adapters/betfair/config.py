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
    cert_dir : str, optional
        The local directory that contains the Betfair certificates.
    instrument_config : BetfairInstrumentProviderConfig, None
        The Betfair instrument provider config.
    subscription_delay_secs : PositiveInt, default 3
        The delay (seconds) before sending the *initial* subscription message.
    keep_alive_secs : PositiveInt, default 36_000 (10 hours)
        The keep alive interval (seconds) for the HTTP client.
    stream_conflate_ms : PositiveInt, optional
        The Betfair data stream conflation setting. Default of `None` means no explicit value is
        set for the conflation interval. Betfair interprets this as using its default behaviour for
        conflation. The default typically applies conflation, so you need to ensure
        stream_conflate_ms=0 is explicitly set to guarantee no conflation.

    """

    account_currency: str
    username: str | None = None
    password: str | None = None
    app_key: str | None = None
    certs_dir: str | None = None
    instrument_config: BetfairInstrumentProviderConfig | None = None
    subscription_delay_secs: PositiveInt | None = 3
    keep_alive_secs: PositiveInt = 36_000  # 10 hours
    stream_conflate_ms: PositiveInt | None = None


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
        The local directory that contains the Betfair certificates.
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
    ignore_external_orders : bool, default False
        If True, orders received over the stream that aren't found in the cache
        will be silently ignored. This is useful when multiple trading nodes
        share the same Betfair account across different markets.

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
    ignore_external_orders: bool = False
