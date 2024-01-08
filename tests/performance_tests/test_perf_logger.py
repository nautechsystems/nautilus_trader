# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Any

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter


def test_logging(benchmark: Any) -> None:
    def run():
        logger = Logger(
            clock=TestClock(),
            level_stdout=LogLevel.ERROR,
            bypass=True,
        )
        logger_adapter = LoggerAdapter(component_name="TEST_LOGGER", logger=logger)

        for i in range(20):
            logger_adapter.error(f"{i}")

    benchmark.pedantic(run, rounds=1_000_000, iterations=10, warmup_rounds=1)
