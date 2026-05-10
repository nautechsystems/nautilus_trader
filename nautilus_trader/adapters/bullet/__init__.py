# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Bullet.xyz perpetuals exchange adapter.

Provides instrument provider, data and execution clients, configurations,
and constants for connecting to Bullet.xyz (mainnet, testnet, staging).
"""

from nautilus_trader.adapters.bullet.config import BulletDataClientConfig
from nautilus_trader.adapters.bullet.config import BulletExecClientConfig
from nautilus_trader.adapters.bullet.constants import BULLET
from nautilus_trader.adapters.bullet.constants import BULLET_CLIENT_ID
from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.adapters.bullet.enums import BulletEnvironment
from nautilus_trader.adapters.bullet.factories import BulletLiveDataClientFactory
from nautilus_trader.adapters.bullet.factories import BulletLiveExecClientFactory
from nautilus_trader.adapters.bullet.providers import BulletInstrumentProvider


__all__ = [
    "BULLET",
    "BULLET_CLIENT_ID",
    "BULLET_VENUE",
    "BulletDataClientConfig",
    "BulletEnvironment",
    "BulletExecClientConfig",
    "BulletInstrumentProvider",
    "BulletLiveDataClientFactory",
    "BulletLiveExecClientFactory",
]
