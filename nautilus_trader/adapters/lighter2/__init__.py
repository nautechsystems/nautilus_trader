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
Lighter cryptocurrency exchange integration adapter.

This subpackage provides an instrument provider, data and execution clients,
configurations, and constants for connecting to and interacting with Lighter's API.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.lighter2``.

"""
from nautilus_trader.adapters.lighter2.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter2.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter2.constants import LIGHTER
from nautilus_trader.adapters.lighter2.constants import LIGHTER_CLIENT_ID
from nautilus_trader.adapters.lighter2.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter2.factories import LighterLiveDataClientFactory
from nautilus_trader.adapters.lighter2.factories import LighterLiveExecClientFactory
from nautilus_trader.adapters.lighter2.providers import LighterInstrumentProvider


__all__ = [
    "LIGHTER",
    "LIGHTER_CLIENT_ID",
    "LIGHTER_VENUE",
    "LighterDataClientConfig",
    "LighterExecClientConfig",
    "LighterInstrumentProvider",
    "LighterLiveDataClientFactory",
    "LighterLiveExecClientFactory",
]