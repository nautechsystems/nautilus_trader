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
        for chunk in range(count):
            time.sleep(random.random() / 5)  # Simulate loading / processing some data # noqa: S311
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
