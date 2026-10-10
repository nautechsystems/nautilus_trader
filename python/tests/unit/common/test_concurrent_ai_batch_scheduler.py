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
Unit tests for ConcurrentAIBatchScheduler and PrefetchPipeline.
"""

from __future__ import annotations

import concurrent.futures
import sys
from pathlib import Path


# Ensure repository root is on sys.path
repo_root = Path(__file__).resolve().parents[4]
if str(repo_root) not in sys.path:
    sys.path.insert(0, str(repo_root))

from examples.backtest.concurrent_ai_batch_scheduler import ConcurrentAIBatchScheduler
from examples.backtest.concurrent_ai_batch_scheduler import CPUVectorizedKernel
from examples.backtest.concurrent_ai_batch_scheduler import PrefetchPipeline


def test_cpu_vectorized_kernel_batch_execution() -> None:
    """
    Test CPUVectorizedKernel executes batched GEMM with correct dimensions.
    """
    kernel = CPUVectorizedKernel(in_features=4, out_features=2)
    inputs = [
        [0.1, 0.2, 0.3, 0.4],
        [0.5, 0.6, 0.7, 0.8],
    ]
    outputs = kernel.execute_batch(inputs)
    assert len(outputs) == 2
    assert len(outputs[0]) == 2
    assert all(0.0 <= val <= 1.0 for row in outputs for val in row)


def test_batch_scheduler_flush_on_capacity() -> None:
    """
    Test ConcurrentAIBatchScheduler flushes immediately when max batch size is reached.
    """
    processed_batches: list[int] = []

    def mock_processor(batch: list[list[float]]) -> list[float]:
        processed_batches.append(len(batch))
        return [sum(row) for row in batch]

    scheduler = ConcurrentAIBatchScheduler(
        batch_processor=mock_processor,
        max_batch_size=4,
        max_latency_ms=100.0,
    )

    futures: list[concurrent.futures.Future] = [
        scheduler.submit(i, [float(i), 1.0]) for i in range(4)
    ]

    results = [fut.result(timeout=1.0) for fut in futures]
    assert len(results) == 4
    assert processed_batches == [4]
    assert [r.prediction for r in results] == [1.0, 2.0, 3.0, 4.0]

    scheduler.shutdown()


def test_batch_scheduler_flush_on_timeout() -> None:
    """
    Test ConcurrentAIBatchScheduler flushes after deadline when batch is partially filled.
    """
    processed_batches: list[int] = []

    def mock_processor(batch: list[list[float]]) -> list[float]:
        processed_batches.append(len(batch))
        return [1.0 for _ in batch]

    scheduler = ConcurrentAIBatchScheduler(
        batch_processor=mock_processor,
        max_batch_size=10,
        max_latency_ms=10.0,
    )

    f1 = scheduler.submit(1, [0.1, 0.2])
    f2 = scheduler.submit(2, [0.3, 0.4])

    res1 = f1.result(timeout=1.0)
    res2 = f2.result(timeout=1.0)

    assert res1.prediction == 1.0
    assert res2.prediction == 1.0
    assert processed_batches == [2]

    scheduler.shutdown()


def test_batch_scheduler_handles_processor_exception() -> None:
    """
    Test ConcurrentAIBatchScheduler propagates exceptions to futures on kernel errors.
    """

    def failing_processor(batch: list[list[float]]) -> list[float]:
        raise RuntimeError("Kernel computation fault")

    scheduler = ConcurrentAIBatchScheduler(
        batch_processor=failing_processor,
        max_batch_size=2,
        max_latency_ms=5.0,
    )

    f = scheduler.submit(1, [0.1])
    try:
        f.result(timeout=1.0)
    except RuntimeError as exc:
        assert "Kernel computation fault" in str(exc)  # noqa: PT017
    else:
        raise AssertionError("Expected RuntimeError was not raised")
    finally:
        scheduler.shutdown()


def test_prefetch_pipeline() -> None:
    """
    Test PrefetchPipeline double-buffering prefetching and consumption.
    """

    def mock_processor(batch: list[list[float]]) -> list[float]:
        return [row[0] * 2.0 for row in batch]

    scheduler = ConcurrentAIBatchScheduler(
        batch_processor=mock_processor,
        max_batch_size=4,
        max_latency_ms=5.0,
    )
    pipeline = PrefetchPipeline(scheduler=scheduler, prefetch_depth=4)

    pipeline.prefetch(10, [5.0])
    pipeline.prefetch(20, [10.0])

    res1 = pipeline.consume(timeout=1.0)
    res2 = pipeline.consume(timeout=1.0)

    assert res1.request_id == 10
    assert res1.prediction == 10.0
    assert res2.request_id == 20
    assert res2.prediction == 20.0

    scheduler.shutdown()


if __name__ == "__main__":
    test_cpu_vectorized_kernel_batch_execution()
    test_batch_scheduler_flush_on_capacity()
    test_batch_scheduler_flush_on_timeout()
    test_batch_scheduler_handles_processor_exception()
    test_prefetch_pipeline()
