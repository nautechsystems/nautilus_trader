import contextlib


try:
    import distributed
except ImportError:
    distributed = None


def running_on_dask() -> bool:
    try:
        from distributed import get_client

        get_client()
        return True
    except (ImportError, ValueError):
        return False


@contextlib.contextmanager
def distributed_lock(name):
    with distributed.Lock(name=name):
        yield


@contextlib.contextmanager
def named_lock(name):
    if running_on_dask():
        with distributed_lock(name=name):
            yield
    else:
        # Nothing to do - sync program
        yield
