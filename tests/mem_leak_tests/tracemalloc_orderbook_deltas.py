#!/usr/bin/env python3

from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.mem_leak_tests.conftest import snapshot_memory


@snapshot_memory(4000)
def run_comprehensive(*args, **kwargs):
    # Create the stub Cython objects
    delta = TestDataStubs.order_book_delta()
    deltas = OrderBookDeltas(delta.instrument_id, deltas=[delta] * 1024)

    # Check printing Cython objects doesn't leak
    repr(deltas.deltas)
    repr(deltas)

    # Convert to pyo3 objects
    pyo3_deltas = deltas.to_pyo3()

    # Convert to capsule
    capsule = pyo3_deltas.as_pycapsule()

    # Convert from capsule back to Cython objects
    deltas = capsule_to_data(capsule)

    # Check printing Cython and pyo3 objects doesn't leak
    repr(pyo3_deltas)
    repr(deltas)


if __name__ == "__main__":
    run_comprehensive()
