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

from nautilus_trader.config import LiveDataClientConfig


class TardisDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``TardisDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The Tardis API secret key.
        If ``None`` then will source the `TARDIS_API_KEY` environment variable.

    References
    ----------
    See the list of Tardis-supported exchanges https://api.tardis.dev/v1/exchanges.

    """

    api_key: str | None = None
