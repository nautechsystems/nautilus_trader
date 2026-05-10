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

This package currently exists as the Python compatibility layer for the v1/Cython
`TradingNode` path. It wraps the Rust Interactive Brokers adapter behind Python-facing
classes and exposes explicit compatibility factories for the legacy live client stack.

For Nautilus v2, the desired boundary is different: the Interactive Brokers adapter
should be consumed directly from `nautilus_trader.core.nautilus_pyo3.interactive_brokers`
without depending on Python adapter logic from this package.

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
from nautilus_trader.adapters.interactive_brokers_pyo3.data import InteractiveBrokersDataClient
from nautilus_trader.adapters.interactive_brokers_pyo3.execution import (
    InteractiveBrokersExecutionClient,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.factories import (
    InteractiveBrokersLiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.factories import (
    InteractiveBrokersLiveExecClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.factories import (
    InteractiveBrokersV1LiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.factories import (
    InteractiveBrokersV1LiveExecClientFactory,
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
    "InteractiveBrokersDataClient",
    "InteractiveBrokersDataClientConfig",
    "InteractiveBrokersExecClientConfig",
    "InteractiveBrokersExecutionClient",
    "InteractiveBrokersInstrumentProvider",
    "InteractiveBrokersInstrumentProviderConfig",
    "InteractiveBrokersLiveDataClientFactory",
    "InteractiveBrokersLiveExecClientFactory",
    "InteractiveBrokersV1LiveDataClientFactory",
    "InteractiveBrokersV1LiveExecClientFactory",
]
