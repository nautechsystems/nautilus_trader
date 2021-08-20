import contextlib
import fcntl
import os

import distributed


# https://stackoverflow.com/questions/56813059/named-multiprocessing-lock


class LocalLock:
    def __init__(self, name):
        self.path = f"./{name}.lock"

    def __enter__(self):
        self.fp = open(self.path, "wb")
        fcntl.flock(self.fp.fileno(), fcntl.LOCK_EX)

    def __exit__(self, _type, value, tb):
        fcntl.flock(self.fp.fileno(), fcntl.LOCK_UN)
        self.fp.close()
        os.unlink(self.path)


def running_on_dask() -> bool:
    try:
        from distributed import get_client

        get_client()
        return True
    except (ImportError, ValueError):
        return False


@contextlib.contextmanager
def named_lock(name):
    lock_cls = distributed.Lock if running_on_dask() else LocalLock
    lock = lock_cls(name=name)
    with lock:
        yield
