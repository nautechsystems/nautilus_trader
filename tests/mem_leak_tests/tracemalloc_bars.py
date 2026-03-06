#!/usr/bin/env python3

from nautilus_trader.model.data import Bar
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.mem_leak_tests.conftest import snapshot_memory


@snapshot_memory(4000)
def run_repr(*args, **kwargs):
    bar = TestDataStubs.bar_5decimal()
    repr(bar)


@snapshot_memory(4000)
def run_from_pyo3(*args, **kwargs):
    pyo3_bar = TestDataProviderPyo3.bar_5decimal()
    Bar.from_pyo3(pyo3_bar)


if __name__ == "__main__":
    run_repr()
    run_from_pyo3()
