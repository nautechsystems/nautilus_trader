#!/usr/bin/env python3

from nautilus_trader.model.data import QuoteTick
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.mem_leak_tests.conftest import snapshot_memory


@snapshot_memory(4000)
def run_repr(*args, **kwargs):
    quote = TestDataStubs.quote_tick()
    repr(quote)


@snapshot_memory(4000)
def run_from_pyo3(*args, **kwargs):
    pyo3_quote = TestDataProviderPyo3.quote_tick()
    QuoteTick.from_pyo3(pyo3_quote)


if __name__ == "__main__":
    run_repr()
    run_from_pyo3()
