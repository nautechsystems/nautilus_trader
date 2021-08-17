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

import inspect
import sys
from concurrent.futures import Executor
from concurrent.futures import Future
from concurrent.futures import ThreadPoolExecutor
from queue import Queue
from typing import Callable, List


try:
    import distributed
    from distributed.cfexecutor import ClientExecutor

    distributed_installed = True
except ImportError:
    distributed_installed = False


PY37 = sys.version_info < (3, 8)


class SyncExecutor(Executor):
    def submit(self, func, *args, **kwargs):  # pylint: disable=arguments-differ
        """Immediately invokes `func(*args, **kwargs)` and returns a future
        with the result (or exception)."""

        future = Future()

        try:
            result = func(*args, **kwargs)
            future.set_result(result)
        except Exception as e:
            print(e.__traceback__)
            print(f"ERR: {e}")
            future.set_exception(e)

        return future


def _determine_workers(executor):
    if isinstance(executor, ThreadPoolExecutor):
        return executor._max_workers
    if isinstance(executor, SyncExecutor):
        return 1
    elif isinstance(executor, ClientExecutor):
        return len(executor._client.scheduler_info()["workers"])
    else:
        raise TypeError(f"Unknown executor type: {type(executor)}")


def queue_runner(in_q: Queue, out_q: Queue, proc_func: Callable):
    """
    Run function for a thread between and input and output queue.
    Parameters
    ----------
    in_q : AnyQueue
        The input queue
    out_q: AnyQueue
        The output queue
    proc_func: Callable
        The generator function to call on each input value
    """
    while in_q.qsize():
        try:
            x = in_q.get(timeout=0.01)
        except Exception:
            break
        try:
            for result in proc_func(**x):
                if result is not None:
                    out_q.put(result)
        except Exception as e:
            # No error handling - break early
            print(f"ERR: {e}")
            break
    out_q.put(None)


def executor_queue_process(
    inputs: List,
    process_func: Callable,
    output_func: Callable,
    executor: Executor = None,
):
    """
    Producer-consumer like pattern with executor in the middle specifically for handling a generator
    function: `process_func`.

    Utilises queues to block the executors reading too many chunks (limiting memory use), while also allowing easy
    parallelization.
    """
    assert inputs and isinstance(
        inputs[0], dict
    ), f"`inputs` should be List[dict] of kwargs for `process_func`, was: {inputs}"
    if not PY37:
        assert inspect.isgeneratorfunction(process_func)
    executor = executor or ThreadPoolExecutor()
    queue_cls = Queue
    if distributed_installed and isinstance(executor, ClientExecutor):
        queue_cls = distributed.Queue

    # Create 3 queues;
    input_q: Queue = queue_cls()  # The queue work items are placed
    output_q: Queue = (
        queue_cls()
    )  # The queue that workers put items in, as well as their individual sentinel value

    # Load inputs into the input queue
    for f in inputs:
        input_q.put(f)

    # Create a processing queue with size=1 for each executor worker - limit memory usage to 1 chunk per executor
    num_workers = min(len(inputs), _determine_workers(executor))
    with executor as client:
        for _ in range(num_workers):
            client.submit(queue_runner, in_q=input_q, out_q=output_q, proc_func=process_func)

    # Pull results from workers,
    sentinel_count = 0
    results = []
    while sentinel_count < num_workers:
        x = output_q.get()
        if x is not None:
            assert isinstance(
                x, dict
            ), "return value from `process_func` should be dict of kwargs for `output_func`"
            r = output_func(**x)
            if r:
                results.append(r)
        else:
            sentinel_count += 1
    return results
