import logging
import os
import time
from multiprocessing import Process
from multiprocessing import Queue
from queue import Empty

from nautilus_trader.persistence.external.sync import named_lock
from tests.test_kit import PACKAGE_ROOT


logger = logging.getLogger()

TEST_DATA = PACKAGE_ROOT + "/data"


def _sleeper(n_sec: int, name: str, q: Queue):
    prefix = f"{os.getpid()}-{name} -"
    q.put(f"{prefix} starting")
    with named_lock(name):
        q.put(f"{prefix} got lock, sleeping for [{n_sec}]")
        time.sleep(n_sec)
        q.put(f"{prefix} done, releasing lock")


def test_named_lock_local():

    q = Queue()
    results = []
    processes = []

    t0, t1 = 1.0, 0.2

    p1 = Process(target=_sleeper, args=(t0, "test", q))
    p2 = Process(target=_sleeper, args=(t1, "test", q))
    processes += [p1, p2]

    for p in processes:
        p.start()
        time.sleep(0.1)

    for p in processes:
        p.join()

    while True:
        try:
            r = q.get(timeout=0)
            results.append(r)
        except Empty:
            break

    expected = [
        f"{processes[0].pid}-test - starting",
        f"{processes[0].pid}-test - got lock, sleeping for [{t0}]",
        f"{processes[1].pid}-test - starting",
        f"{processes[0].pid}-test - done, releasing lock",
        f"{processes[1].pid}-test - got lock, sleeping for [{t1}]",
        f"{processes[1].pid}-test - done, releasing lock",
    ]
    assert results == expected
