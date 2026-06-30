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

import socket
import time

from nautilus_trader.datadog.telemetry import DatadogTelemetry
from nautilus_trader.datadog.telemetry import DatadogTelemetryConfig
from nautilus_trader.datadog.telemetry import configure
from nautilus_trader.datadog.telemetry import distribution
from nautilus_trader.datadog.telemetry import enabled
from nautilus_trader.datadog.telemetry import stop


def _receive_lines(sock: socket.socket, count: int) -> set[str]:
    deadline = time.monotonic() + 2.0
    lines: set[str] = set()

    while len(lines) < count and time.monotonic() < deadline:
        try:
            payload, _addr = sock.recvfrom(4096)
        except TimeoutError:
            continue
        lines.add(payload.decode("utf-8"))

    return lines


class TestDatadogTelemetry:
    def teardown_method(self):
        stop()

    def test_telemetry_worker_sends_dogstatsd_lines(self):
        # Arrange
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
            receiver.bind(("127.0.0.1", 0))
            receiver.settimeout(0.05)
            _host, port = receiver.getsockname()

            telemetry = DatadogTelemetry(
                DatadogTelemetryConfig(
                    host="127.0.0.1",
                    port=port,
                    namespace="test",
                    constant_tags=("service:nautilus",),
                    flush_interval=0.01,
                ),
            )
            telemetry.start()

            # Act
            telemetry.increment("orders.submitted", tags=("env:test",))
            telemetry.gauge("queue.depth", 4, tags=("queue:data",))
            telemetry.distribution("quote.age_ms", 1.5, tags=("instrument:BTCUSDT",))
            lines = _receive_lines(receiver, 3)
            telemetry.close()

        # Assert
        assert "test.orders.submitted:1|c|#service:nautilus,env:test" in lines
        assert "test.queue.depth:4|g|#service:nautilus,queue:data" in lines
        assert "test.quote.age_ms:1.5|d|#service:nautilus,instrument:BTCUSDT" in lines
        assert telemetry.stats().sent == 3

    def test_global_telemetry_can_be_configured_and_stopped(self):
        # Arrange
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as receiver:
            receiver.bind(("127.0.0.1", 0))
            receiver.settimeout(0.05)
            _host, port = receiver.getsockname()

            config = DatadogTelemetryConfig(
                host="127.0.0.1",
                port=port,
                namespace="test",
                flush_interval=0.01,
            )

            # Act
            configure(config)
            distribution("quote.internal_age_ms", 2.0, tags=("env:test",))
            lines = _receive_lines(receiver, 1)
            stop()

        # Assert
        assert enabled() is False
        assert "test.quote.internal_age_ms:2.0|d|#env:test" in lines
