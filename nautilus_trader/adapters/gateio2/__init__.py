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
Gate.io integration adapter for Nautilus Trader.

This adapter provides connectivity to the Gate.io cryptocurrency exchange,
supporting spot, futures, and other markets.
"""

from nautilus_trader.adapters.gateio2.config import GateioDataClientConfig
from nautilus_trader.adapters.gateio2.config import GateioExecClientConfig
from nautilus_trader.adapters.gateio2.factories import GateioLiveDataClientFactory
from nautilus_trader.adapters.gateio2.factories import GateioLiveExecClientFactory
from nautilus_trader.adapters.gateio2.providers import GateioInstrumentProvider


__all__ = [
    "GateioDataClientConfig",
    "GateioExecClientConfig",
    "GateioInstrumentProvider",
    "GateioLiveDataClientFactory",
    "GateioLiveExecClientFactory",
]
