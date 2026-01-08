# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Benchmarks comparing identifier types for creation, hashing, and equality.

Run with: pytest tests/benchmarks/test_identifier_comparison.py -v --benchmark-only

"""

import pytest

from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId


SHORT = "BINANCE"
MEDIUM = "O-20231215-001-001"
LONG = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"  # 36 chars (TradeId max)


def test_symbol_equality(benchmark):
    symbol = Symbol("AUD/USD")

    def symbol_equality() -> bool:
        return symbol == symbol

    benchmark(symbol_equality)


def test_venue_equality(benchmark):
    venue = Venue("SIM")

    def venue_equality() -> bool:
        return venue == venue

    benchmark(venue_equality)


# =============================================================================
# Creation benchmarks
# =============================================================================


@pytest.mark.benchmark(group="creation")
def test_client_order_id_create_short(benchmark):
    benchmark(ClientOrderId, SHORT)


@pytest.mark.benchmark(group="creation")
def test_client_order_id_create_medium(benchmark):
    benchmark(ClientOrderId, MEDIUM)


@pytest.mark.benchmark(group="creation")
def test_client_order_id_create_long(benchmark):
    benchmark(ClientOrderId, LONG)


@pytest.mark.benchmark(group="creation")
def test_venue_order_id_create_short(benchmark):
    benchmark(VenueOrderId, SHORT)


@pytest.mark.benchmark(group="creation")
def test_venue_order_id_create_medium(benchmark):
    benchmark(VenueOrderId, MEDIUM)


@pytest.mark.benchmark(group="creation")
def test_venue_order_id_create_long(benchmark):
    benchmark(VenueOrderId, LONG)


@pytest.mark.benchmark(group="creation")
def test_trade_id_create_short(benchmark):
    benchmark(TradeId, SHORT)


@pytest.mark.benchmark(group="creation")
def test_trade_id_create_medium(benchmark):
    benchmark(TradeId, MEDIUM)


@pytest.mark.benchmark(group="creation")
def test_trade_id_create_long(benchmark):
    benchmark(TradeId, LONG)


# =============================================================================
# Equality benchmarks (same)
# =============================================================================


@pytest.mark.benchmark(group="eq_same")
def test_client_order_id_eq_same_short(benchmark):
    a = ClientOrderId(SHORT)
    b = ClientOrderId(SHORT)
    benchmark(lambda: a == b)


@pytest.mark.benchmark(group="eq_same")
def test_client_order_id_eq_same_medium(benchmark):
    a = ClientOrderId(MEDIUM)
    b = ClientOrderId(MEDIUM)
    benchmark(lambda: a == b)


@pytest.mark.benchmark(group="eq_same")
def test_client_order_id_eq_same_long(benchmark):
    a = ClientOrderId(LONG)
    b = ClientOrderId(LONG)
    benchmark(lambda: a == b)


# =============================================================================
# Equality benchmarks (different)
# =============================================================================


@pytest.mark.benchmark(group="eq_diff")
def test_client_order_id_eq_diff_short(benchmark):
    a = ClientOrderId("BINANCE")
    b = ClientOrderId("POLYGON")
    benchmark(lambda: a == b)


@pytest.mark.benchmark(group="eq_diff")
def test_client_order_id_eq_diff_medium(benchmark):
    a = ClientOrderId("O-20231215-001-001")
    b = ClientOrderId("O-20231215-001-002")
    benchmark(lambda: a == b)


@pytest.mark.benchmark(group="eq_diff")
def test_client_order_id_eq_diff_long(benchmark):
    a = ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5-C6400")
    b = ClientOrderId("O-20250725-080046-TEST-000-1-LEG-E1AQ5-C6401")
    benchmark(lambda: a == b)


# =============================================================================
# Hash benchmarks
# =============================================================================


@pytest.mark.benchmark(group="hash")
def test_client_order_id_hash_short(benchmark):
    a = ClientOrderId(SHORT)
    benchmark(hash, a)


@pytest.mark.benchmark(group="hash")
def test_client_order_id_hash_medium(benchmark):
    a = ClientOrderId(MEDIUM)
    benchmark(hash, a)


@pytest.mark.benchmark(group="hash")
def test_client_order_id_hash_long(benchmark):
    a = ClientOrderId(LONG)
    benchmark(hash, a)


# =============================================================================
# Realistic workload: create and find in collection
# =============================================================================


@pytest.mark.benchmark(group="realistic")
def test_client_order_id_create_1000(benchmark):
    ids = [f"O-20231215-{i:03d}-{j:03d}" for i in range(10) for j in range(100)]

    def create_all():
        return [ClientOrderId(s) for s in ids]

    benchmark(create_all)


@pytest.mark.benchmark(group="realistic")
def test_client_order_id_find_in_1000(benchmark):
    ids = [ClientOrderId(f"O-20231215-{i:03d}-{j:03d}") for i in range(10) for j in range(100)]
    target = ClientOrderId("O-20231215-005-050")

    def find():
        for id_ in ids:
            if id_ == target:
                return id_
        return None

    benchmark(find)


@pytest.mark.benchmark(group="realistic")
def test_client_order_id_dict_lookup(benchmark):
    ids = {
        ClientOrderId(f"O-20231215-{i:03d}-{j:03d}"): i * 100 + j
        for i in range(10)
        for j in range(100)
    }
    target = ClientOrderId("O-20231215-005-050")

    benchmark(lambda: ids.get(target))
