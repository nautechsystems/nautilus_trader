# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import random
import time
from concurrent.futures import ThreadPoolExecutor

import pytest
from distributed import Client
from distributed import LocalCluster
from distributed.cfexecutor import ClientExecutor

from nautilus_trader.persistence.backtest.processing import SyncExecutor
from nautilus_trader.persistence.backtest.processing import _determine_workers
from nautilus_trader.persistence.backtest.processing import executor_queue_process


def test_determine_workers():
    assert _determine_workers(SyncExecutor()) == 1
    assert _determine_workers(ThreadPoolExecutor(max_workers=2)) == 2
    assert _determine_workers(ClientExecutor(Client(LocalCluster(n_workers=4)))) == 4


@pytest.mark.parametrize(
    "executor_cls", (SyncExecutor, ThreadPoolExecutor, lambda: ClientExecutor(Client()))
)
def test_executor_process(executor_cls):
    def process(name: str, count: int):
        # Simulate loading / processing some data
        for chunk in range(count):
            time.sleep(random.random() / 5)  # noqa: S311, B311
            yield {"x": f"{name}-{chunk}"}

    results = []

    def append(x):
        results.append(x)

    inputs = [
        {"name": "a", "count": 3},
        {"name": "b", "count": 5},
        {"name": "c", "count": 1},
    ]
    executor = executor_cls()
    executor_queue_process(
        executor=executor, inputs=inputs, process_func=process, output_func=append
    )

    # Ensure no chunks arrive out of order
    for key in ("a", "b", "c"):
        values = [x for x in results if x.startswith(key)]
        assert values and values == sorted(values)
