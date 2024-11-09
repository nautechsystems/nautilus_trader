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

from functools import lru_cache

from nautilus_trader.adapters.tardis.providers import TardisInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import TardisHttpClient


@lru_cache(1)
def get_tardis_http_client(
    api_key: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 60,
) -> TardisHttpClient:
    """
    Cache and return a Tardis HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that cached
    client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The Tardis API key for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, default 60
        The timeout (seconds) for HTTP requests to Tardis.

    Returns
    -------
    TardisHttpClient

    """
    return TardisHttpClient(
        api_key=api_key,
        base_url=base_url,
        timeout_secs=timeout_secs,
    )


@lru_cache(1)
def get_tardis_instrument_provider(
    client: TardisHttpClient,
    config: InstrumentProviderConfig,
) -> TardisInstrumentProvider:
    """
    Cache and return a Tardis instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : TardisHttpClient
        The client for the instrument provider.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    TardisInstrumentProvider

    """
    return TardisInstrumentProvider(
        client=client,
        config=config,
    )
