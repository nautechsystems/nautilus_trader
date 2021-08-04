import inspect
import os
import pathlib
from concurrent.futures import Executor
from concurrent.futures import Future
from concurrent.futures import ThreadPoolExecutor
from queue import Queue
from threading import Thread
from typing import Callable, List, Union

import fsspec.utils


try:
    import distributed
    from distributed.cfexecutor import ClientExecutor
except ImportError:
    pass


KEY = "NAUTILUS_DATA"

AnyQueue = Union[Queue, distributed.Queue]


def _path() -> str:
    if KEY not in os.environ:
        raise KeyError("`NAUTILUS_DATA` env variable not set")
    return os.environ[KEY]


def get_catalog_fs() -> fsspec.AbstractFileSystem:
    url = _path()
    protocol = fsspec.utils.get_protocol(url)
    return fsspec.filesystem(
        protocol=protocol,
    )


def get_catalog_root() -> pathlib.Path:
    url = _path()
    protocol = fsspec.utils.get_protocol(url)
    root = pathlib.Path(url.replace(f"{protocol}://", ""))
    for dir in ("data",):
        root.joinpath(dir).mkdir(exist_ok=True)
    return root


class SyncExecutor(Executor):
    def submit(self, fn, *args, **kwargs):  # pylint: disable=arguments-differ
        """Immediately invokes `fn(*args, **kwargs)` and returns a future
        with the result (or exception)."""

        future = Future()

        try:
            result = fn(*args, **kwargs)
            future.set_result(result)
        except Exception as e:
            future.set_exception(e)

        return future


def push(in_q, out_q):
    while True:
        x = in_q.get()
        out_q.put(x)


def merge_queues(*in_qs, **kwargs):
    """Merge multiple queues together

    >>> out_q = merge(q1, q2, q3)
    """
    out_q = Queue(**kwargs)
    threads = [Thread(target=push, args=(q, out_q)) for q in in_qs]
    for t in threads:
        t.daemon = True
        t.start()
    return out_q


def _determine_workers(executor):
    if isinstance(executor, ThreadPoolExecutor):
        return executor._max_workers
    if isinstance(executor, SyncExecutor):
        return 1
    elif isinstance(executor, ClientExecutor):
        return len(executor._client.scheduler_info()["workers"])
    else:
        raise TypeError(f"Unknown executor type: {type(executor)}")


def queue_runner(in_q: AnyQueue, out_q: AnyQueue, func: Callable):
    """
    Run function for a thread between and input and output queue.
    Parameters
    ----------
    in_q : AnyQueue
        The input queue
    out_q: AnyQueue
        The output queue
    func: Callable
        The generator function to call on each input value
    """
    while in_q.qsize():
        x = in_q.get(block=False)
        if x is None:
            continue
        try:
            for result in func(**x):
                if result is not None:
                    out_q.put(result)
        except Exception as e:
            # No error handling - break early
            print(f"ERR: {e}")
            out_q.put(None)
            return
    out_q.put(None)


def executor_queue_process(
    executor: Executor, inputs: List, process_func: Callable, output_func: Callable, progress=True
) -> Union[Queue, distributed_Queue]:
    """
    Producer-consumer like pattern with executor in the middle specifically for handling a generator
    function: `process_func`.

    Utilises queues to block the executors reading too many chunks (limiting memory use), while also allowing easy
    parallelization.
    """
    assert inspect.isgeneratorfunction(process_func)
    executor = executor or ThreadPoolExecutor()
    queue_cls = Queue if not isinstance(executor, ClientExecutor) else distributed.Queue

    # Create input and output queues
    input_q = queue_cls()
    output_q = queue_cls()

    # Load inputs into the input queue
    for f in inputs:
        input_q.put(f)

    # Create a processing queue with size=1 for each executor worker - limit memory usage to 1 chunk per executor
    num_workers = min(len(inputs), _determine_workers(executor))
    with executor as client:
        for _ in range(num_workers):
            client.submit(queue_runner, in_q=input_q, out_q=output_q, proc_func=process_func)

    sentinel_count = 0
    while sentinel_count < num_workers:
        result = output_q.get()
        if result is not None:
            output_func(result)
        else:
            sentinel_count += 1
