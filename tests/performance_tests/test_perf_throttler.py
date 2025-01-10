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

import pandas as pd
import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Throttler


def buffering_throttler(name: str, limit: int) -> Throttler:
    handler: list[str] = []
    return Throttler(
        name=name,
        limit=limit,
        interval=pd.Timedelta(seconds=1),
        output_send=handler.append,
        output_drop=None,
        clock=LiveClock(),
    )


@pytest.mark.skip()
def test_send_unlimited(benchmark):
    throttler = buffering_throttler("buffer-1", 10_000)
    benchmark(throttler.send, "MESSAGE")
