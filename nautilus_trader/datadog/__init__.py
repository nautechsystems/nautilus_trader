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
Datadog telemetry helpers for NautilusTrader.

The runtime telemetry path is disabled by default and becomes active only after
``configure`` is called.
"""

from nautilus_trader.datadog.dashboard import load_dashboard
from nautilus_trader.datadog.dashboard import publish_dashboard
from nautilus_trader.datadog.telemetry import DatadogTelemetry
from nautilus_trader.datadog.telemetry import DatadogTelemetryConfig
from nautilus_trader.datadog.telemetry import configure
from nautilus_trader.datadog.telemetry import distribution
from nautilus_trader.datadog.telemetry import enabled
from nautilus_trader.datadog.telemetry import gauge
from nautilus_trader.datadog.telemetry import histogram
from nautilus_trader.datadog.telemetry import increment
from nautilus_trader.datadog.telemetry import stop


__all__ = [
    "DatadogTelemetry",
    "DatadogTelemetryConfig",
    "configure",
    "distribution",
    "enabled",
    "gauge",
    "histogram",
    "increment",
    "load_dashboard",
    "publish_dashboard",
    "stop",
]
