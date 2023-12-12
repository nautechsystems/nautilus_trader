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
    datasets : list[str], optional
        The datasets to prepare operations for (will be used for 'ALL_SYBOLS' requests).
    instrument_ids : list[InstrumentId], optional
        The instrument IDs to request definitions for on start.
        Databento 'datasets' based on venues will be used for any 'ALL_SYMBOLS' requests.

    """

    api_key: str | None = None
    http_gateway: str | None = None
    live_gateway: str | None = None
    datasets: list[str] | None = None
    instrument_ids: list[InstrumentId] | None = None
