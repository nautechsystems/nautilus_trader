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


@pytest.mark.skip
def test_send_unlimited(benchmark):
    throttler = buffering_throttler("buffer-1", 10_000)
    benchmark(throttler.send, "MESSAGE")
