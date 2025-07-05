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

import os
import tempfile
import time

import psutil
import pytest

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_binance_snapshot5
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_binance_snapshot25
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_bitmex_trades
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_deribit_book_l2
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_huobi_quotes


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_deltas(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_deribit_book_l2()
    instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")  # Override instrument in data
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
        instrument_id=instrument_id,
    )

    # Act
    deltas = loader.load_deltas(filepath, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == instrument_id
    assert deltas[0].action == BookAction.ADD
    assert deltas[0].order.side == OrderSide.SELL
    assert deltas[0].order.price == Price.from_str("6421.5")
    assert deltas[0].order.size == Quantity.from_str("18640")
    assert deltas[0].flags == 0
    assert deltas[0].sequence == 0
    assert deltas[0].ts_event == 1585699200245000000
    assert deltas[0].ts_init == 1585699200355684000


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_depth10_from_snapshot5(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_binance_snapshot5()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    deltas = loader.load_depth10(filepath, levels=5, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT.BINANCE")
    assert len(deltas[0].bids) == 10
    assert deltas[0].bids[0].price == Price.from_str("11657.07")
    assert deltas[0].bids[0].size == Quantity.from_str("10.896")
    assert deltas[0].bids[0].side == OrderSide.BUY
    assert deltas[0].bids[0].order_id == 0
    assert len(deltas[0].asks) == 10
    assert deltas[0].asks[0].price == Price.from_str("11657.08")
    assert deltas[0].asks[0].size == Quantity.from_str("1.714")
    assert deltas[0].asks[0].side == OrderSide.SELL
    assert deltas[0].asks[0].order_id == 0
    assert deltas[0].bid_counts[0] == 1
    assert deltas[0].ask_counts[0] == 1
    assert deltas[0].flags == 128
    assert deltas[0].ts_event == 1598918403696000000
    assert deltas[0].ts_init == 1598918403810979000
    assert deltas[0].sequence == 0


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_depth10_from_snapshot25(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_binance_snapshot25()
    instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BINANCE")  # Override instrument in data
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
        instrument_id=instrument_id,
    )

    # Act
    deltas = loader.load_depth10(filepath, levels=25, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
    assert len(deltas[0].bids) == 10
    assert deltas[0].bids[0].price == Price.from_str("11657.07")
    assert deltas[0].bids[0].size == Quantity.from_str("10.896")
    assert deltas[0].bids[0].side == OrderSide.BUY
    assert deltas[0].bids[0].order_id == 0
    assert len(deltas[0].asks) == 10
    assert deltas[0].asks[0].price == Price.from_str("11657.08")
    assert deltas[0].asks[0].size == Quantity.from_str("1.714")
    assert deltas[0].asks[0].side == OrderSide.SELL
    assert deltas[0].asks[0].order_id == 0
    assert deltas[0].bid_counts[0] == 1
    assert deltas[0].ask_counts[0] == 1
    assert deltas[0].flags == 128
    assert deltas[0].ts_event == 1598918403696000000
    assert deltas[0].ts_init == 1598918403810979000
    assert deltas[0].sequence == 0


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 0],
        [1, None],
        [1, 0],
    ],
)
def test_tardis_load_quotes(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_huobi_quotes()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    trades = loader.load_quotes(filepath, limit=10_000)

    # Assert
    assert len(trades) == 10_000
    assert trades[0].instrument_id == InstrumentId.from_str("BTC-USD.HUOBI_DELIVERY")
    assert trades[0].bid_price == Price.from_str("8629.2")
    assert trades[0].ask_price == Price.from_str("8629.3")
    assert trades[0].bid_size == Quantity.from_str("806")
    assert trades[0].ask_size == Quantity.from_str("5494")
    assert trades[0].ts_event == 1588291201099000000
    assert trades[0].ts_init == 1588291201234268000


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 0],
        [1, None],
        [1, 0],
    ],
)
def test_tardis_load_trades(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_bitmex_trades()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    trades = loader.load_trades(filepath, limit=10_000)

    # Assert
    assert len(trades) == 10_000
    assert trades[0].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert trades[0].price == Price.from_str("8531.5")
    assert trades[0].size == Quantity.from_str("2152")
    assert trades[0].aggressor_side == AggressorSide.SELLER
    assert trades[0].trade_id == TradeId("ccc3c1fa-212c-e8b0-1706-9b9c4f3d5ecf")
    assert trades[0].ts_event == 1583020803145000000
    assert trades[0].ts_init == 1583020803307160000


def test_dynamic_precision_inference_optimization():
    """
    Test that dynamic precision inference works correctly with memory optimization.
    """
    # Create synthetic CSV data with increasing precision to test the optimization
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0
binance-futures,BTCUSDT,1640995204000000,1640995204100000,false,ask,50000.1234,0.5"""

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Monitor memory usage
        process = psutil.Process(os.getpid())
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB

        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
        )

        start_time = time.time()
        deltas = loader.load_deltas(temp_file)
        elapsed_time = time.time() - start_time

        current_memory = process.memory_info().rss / 1024 / 1024  # MB
        memory_used = current_memory - initial_memory

        # Verify basic functionality
        assert len(deltas) == 5

        # Key test: All deltas should have the same (maximum) precision
        # This verifies that early records were updated when precision increased
        expected_price_precision = 4  # From 50000.1234
        expected_size_precision = 1  # From 1.5, 0.5

        for i, delta in enumerate(deltas):
            assert (
                delta.order.price.precision == expected_price_precision
            ), f"Delta {i} price precision should be {expected_price_precision}"
            assert (
                delta.order.size.precision == expected_size_precision
            ), f"Delta {i} size precision should be {expected_size_precision}"

        # Performance check - should be very fast for small dataset
        assert elapsed_time < 1.0, f"Loading took too long: {elapsed_time:.3f}s"

        # Memory should be reasonable for 5 records
        assert memory_used < 50, f"Memory usage too high: {memory_used:.2f} MB"

        print(
            f"✓ Dynamic precision test: {len(deltas)} deltas in {elapsed_time:.3f}s, {memory_used:.2f} MB",
        )

    finally:
        # Clean up
        os.unlink(temp_file)
