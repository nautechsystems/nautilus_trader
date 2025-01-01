#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

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
