#!/usr/bin/env python3

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.mem_leak_tests.conftest import snapshot_memory


@snapshot_memory(4000)
def run_repr(*args, **kwargs):
    depth = TestDataStubs.order_book_depth10()
    repr(depth)  # Copies bids and asks book order data from Rust on every iteration


@snapshot_memory(4000)
def run_from_pyo3(*args, **kwargs):
    pyo3_depth = nautilus_pyo3.OrderBookDepth10.get_stub()
    OrderBookDepth10.from_pyo3(pyo3_depth)


if __name__ == "__main__":
    run_repr()
    run_from_pyo3()
