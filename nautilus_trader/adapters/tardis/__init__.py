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
"""
Tardis crypto market data integration adapter https://tardis.dev/.

This subpackage provides instrument providers, data client configuration,
factories, constants, and data loaders for connecting to and interacting with
Tardis APIs and the Tardis Machine server.

For convenience, the most commonly used symbols are re-exported at the subpackage's
top level, so downstream code can simply import from ``nautilus_trader.adapters.tardis``.

"""
from nautilus_trader.adapters.tardis.config import TardisDataClientConfig
from nautilus_trader.adapters.tardis.constants import TARDIS
from nautilus_trader.adapters.tardis.constants import TARDIS_CLIENT_ID
from nautilus_trader.adapters.tardis.factories import TardisLiveDataClientFactory
from nautilus_trader.adapters.tardis.factories import get_tardis_http_client
from nautilus_trader.adapters.tardis.factories import get_tardis_instrument_provider
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.adapters.tardis.providers import TardisInstrumentProvider


__all__ = [
    "TARDIS",
    "TARDIS_CLIENT_ID",
    "TardisCSVDataLoader",
    "TardisDataClientConfig",
    "TardisInstrumentProvider",
    "TardisLiveDataClientFactory",
    "get_tardis_http_client",
    "get_tardis_instrument_provider",
]
