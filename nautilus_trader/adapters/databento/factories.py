# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from functools import lru_cache

import databento

from nautilus_trader.adapters.env import get_env_key


DATABENTO_HTTP_CLIENTS: dict[str, databento.Historical] = {}


@lru_cache(1)
def get_cached_databento_http_client(
    key: str | None = None,
    gateway: str | None = None,
) -> databento.Historical:
    """
    Cache and return a Databento historical HTTP client with the given key and gateway.

    If a cached client with matching key and gateway already exists, then that
    cached client will be returned.

    Parameters
    ----------
    key : str, optional
        The Databento API secret key for the client.
    gateway : str, optional
        The Databento historical HTTP client gateway override.

    Returns
    -------
    databento.Historical

    """
    global BINANCE_HTTP_CLIENTS

    key = key or get_env_key("DATABENTO_API_KEY")

    client_key: str = "|".join((key, gateway or ""))
    if client_key not in DATABENTO_HTTP_CLIENTS:
        client = databento.Historical(key=key, gateway=gateway or databento.HistoricalGateway.BO1)
        DATABENTO_HTTP_CLIENTS[client_key] = client
    return DATABENTO_HTTP_CLIENTS[client_key]
