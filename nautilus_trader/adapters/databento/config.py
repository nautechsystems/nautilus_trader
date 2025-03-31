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

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.model.identifiers import InstrumentId


class DatabentoDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``DatabentoDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Databento API secret key.
        If ``None`` then will source the `DATABENTO_API_KEY` environment variable.
    http_gateway : str, optional
        The historical HTTP client gateway override.
    live_gateway : str, optional
        The live client gateway override.
    use_exchange_as_venue : bool, default True
        If the `exchange` field will be used as the venue for instrument IDs.
    timeout_initial_load : float, default 15.0
        The timeout (seconds) to wait for instruments to load (concurrently per dataset).
    mbo_subscriptions_delay : float, default 3.0
        The timeout (seconds) to wait for MBO/L3 subscriptions (concurrently per dataset).
        After the timeout the MBO order book feed will start and replay messages from the initial
        snapshot and then all deltas.
    parent_symbols : dict[str, set[str]], optional
        The Databento parent symbols to subscribe to instrument definitions for on start.
        This is a map of Databento dataset keys -> to a sequence of the parent symbols,
        e.g. {'GLBX.MDP3', ['ES.FUT', 'ES.OPT']} (for all E-mini S&P 500 futures and options products).
    instrument_ids : list[InstrumentId], optional
        The instrument IDs to request instrument definitions for on start.
    venue_dataset_map: dict[str, str], optional
        A dictionary to override the default dataset used for a venue.

    """

    api_key: str | None = None
    http_gateway: str | None = None
    live_gateway: str | None = None
    use_exchange_as_venue: bool = True
    timeout_initial_load: float | None = 15.0
    mbo_subscriptions_delay: float | None = 3.0  # Need to have received all definitions
    instrument_ids: list[InstrumentId] | None = None
    parent_symbols: dict[str, set[str]] | None = None
    venue_dataset_map: dict[str, str] | None = None
