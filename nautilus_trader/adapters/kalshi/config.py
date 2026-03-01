# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
#  KIND, either express or implied. See the License for the specific language governing
#  permissions and limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os

from nautilus_trader.config import LiveDataClientConfig


class KalshiDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``KalshiDataClient`` instances.

    Parameters
    ----------
    base_url : str, optional
        REST base URL. Defaults to production (https://api.elections.kalshi.com/trade-api/v2).
    ws_url : str, optional
        WebSocket URL. Defaults to production (wss://api.elections.kalshi.com/trade-api/ws/v2).
    series_tickers : tuple[str, ...], default ()
        Series tickers to load instruments for, e.g. ``("KXBTC", "PRES-2024")``.
    event_tickers : tuple[str, ...], default ()
        Optional event tickers for finer-grained filtering.
    instrument_reload_interval_mins : int, default 60
        How often to refresh instruments from the API.
    rate_limit_rps : int, default 20
        REST requests per second (Basic tier default).
    api_key_id : str, optional
        Kalshi API key ID.
        If ``None`` then will source the ``KALSHI_API_KEY_ID`` environment variable.
    private_key_pem : str, optional
        RSA private key in PEM format.
        If ``None`` then will source the ``KALSHI_PRIVATE_KEY_PEM`` environment variable.

    """

    base_url: str | None = None
    ws_url: str | None = None
    series_tickers: tuple[str, ...] = ()
    event_tickers: tuple[str, ...] = ()
    instrument_reload_interval_mins: int = 60
    rate_limit_rps: int = 20
    api_key_id: str | None = None
    private_key_pem: str | None = None

    def resolved_api_key_id(self) -> str | None:
        # TODO: consider moving credential resolution into the factory/client
        return self.api_key_id or os.environ.get("KALSHI_API_KEY_ID")

    def resolved_private_key_pem(self) -> str | None:
        # TODO: consider moving credential resolution into the factory/client
        return self.private_key_pem or os.environ.get("KALSHI_PRIVATE_KEY_PEM")

    def has_credentials(self) -> bool:
        return bool(self.resolved_api_key_id() and self.resolved_private_key_pem())
