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
Provides a PyO3-based API integration for Interactive Brokers.

This package exposes the configs, enums, instrument provider, historical client, and
gateway helper from the Rust Interactive Brokers adapter at
`nautilus_trader.core.nautilus_pyo3.interactive_brokers`. The live data and execution
clients are constructed by the Rust live node from configs and are not exposed here.

"""

from nautilus_trader.adapters.interactive_brokers_pyo3.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersDataClientConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersExecClientConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.historical import (
    HistoricalInteractiveBrokersClient,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.historical import (
    HistoricInteractiveBrokersClient,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.providers import (
    InteractiveBrokersInstrumentProvider,
)


__all__ = [
    "DockerizedIBGatewayConfig",
    "HistoricInteractiveBrokersClient",
    "HistoricalInteractiveBrokersClient",
    "InteractiveBrokersDataClientConfig",
    "InteractiveBrokersExecClientConfig",
    "InteractiveBrokersInstrumentProvider",
    "InteractiveBrokersInstrumentProviderConfig",
]
