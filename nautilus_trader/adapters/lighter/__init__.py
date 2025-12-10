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
Lighter Exchange integration adapter.

This subpackage hosts configuration, factories, and constants for the Lighter adapter.
Functional clients will be delivered incrementally per the implementation plan.

"""

from nautilus_trader.adapters.lighter.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter.constants import LIGHTER
from nautilus_trader.adapters.lighter.constants import LIGHTER_CLIENT_ID
from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.adapters.lighter.data import LighterDataClient
from nautilus_trader.adapters.lighter.enums import LighterNetwork
from nautilus_trader.adapters.lighter.execution import LighterExecutionClient
from nautilus_trader.adapters.lighter.factories import LighterLiveDataClientFactory
from nautilus_trader.adapters.lighter.factories import LighterLiveExecClientFactory
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider


__all__ = [
    "LIGHTER",
    "LIGHTER_CLIENT_ID",
    "LIGHTER_VENUE",
    "LighterDataClient",
    "LighterDataClientConfig",
    "LighterExecClientConfig",
    "LighterExecutionClient",
    "LighterInstrumentProvider",
    "LighterLiveDataClientFactory",
    "LighterLiveExecClientFactory",
    "LighterNetwork",
]
