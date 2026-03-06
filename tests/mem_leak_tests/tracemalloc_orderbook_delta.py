#!/usr/bin/env python3

from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.mem_leak_tests.conftest import snapshot_memory


@snapshot_memory(4000)
def run_repr(*args, **kwargs):
    delta = TestDataStubs.order_book_delta()
    repr(delta)  # Copies bids and asks book order data from Rust on every iteration


@snapshot_memory(4000)
def run_from_pyo3(*args, **kwargs):
    pyo3_delta = TestDataProviderPyo3.order_book_delta()
    OrderBookDelta.from_pyo3(pyo3_delta)


if __name__ == "__main__":
    run_repr()
    run_from_pyo3()
