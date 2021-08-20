import os
import time
from multiprocessing import Process

from nautilus_trader.persistence.external.sync import named_lock
from tests.test_kit import PACKAGE_ROOT


TEST_DATA = PACKAGE_ROOT + "/data"


def _sleeper(n_sec: int, name: str):
    prefix = f"{os.getpid()} -"
    print(f"{prefix} starting")
    with named_lock(name):
        print(f"{prefix} got lock, sleeping for [n_sec]")
        time.sleep(n_sec)
        print(f"{prefix} done, releasing lock")


def test_named_lock_local():

    processes = []

    for _ in range(2):
        p1 = Process(
            target=_sleeper,
            args=(
                5,
                "test",
            ),
        )
        p1.start()

        p2 = Process(target=_sleeper, args=(1, "test"))
        p2.start()

        processes += [p1, p2]

    for p in processes:
        p.join()
