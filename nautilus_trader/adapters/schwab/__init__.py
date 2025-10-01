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
Charles Schwab brokerage adapter package.
"""

from nautilus_trader.adapters.schwab.common import SCHWAB
from nautilus_trader.adapters.schwab.common import SCHWAB_VENUE


__all__ = ["SCHWAB", "SCHWAB_VENUE"]

try:  # Optional exports, avoid import-time failures during partial builds
    from nautilus_trader.adapters.schwab.config import SchwabDataClientConfig
    from nautilus_trader.adapters.schwab.config import SchwabExecClientConfig
    from nautilus_trader.adapters.schwab.config import SchwabInstrumentProviderConfig
    from nautilus_trader.adapters.schwab.factories import SchwabLiveDataClientFactory
    from nautilus_trader.adapters.schwab.factories import SchwabLiveExecClientFactory
except ImportError:  # pragma: no cover - executed on environments without compiled deps
    pass
else:
    __all__ += [
        "SchwabDataClientConfig",
        "SchwabExecClientConfig",
        "SchwabInstrumentProviderConfig",
        "SchwabLiveDataClientFactory",
        "SchwabLiveExecClientFactory",
    ]
