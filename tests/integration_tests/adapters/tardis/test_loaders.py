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
import sys
import tempfile
import time
from decimal import Decimal

import psutil
import pytest

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.model.data import FundingRateUpdate
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
from tests.integration_tests.adapters.tardis.conftest import get_test_data_path


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")


def test_csv_loader_with_malformed_data():
    """
    Test CSV loader error handling for malformed data.
    """
    malformed_cases = [
        # Missing required columns
        "exchange,symbol\nbinance,BTCUSDT",
        # Invalid timestamp
        "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount\nbinance,BTCUSDT,invalid,1640995200100000,true,ask,50000.0,1.0",
    ]

    loader = TardisCSVDataLoader()

    for malformed_data in malformed_cases:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            f.write(malformed_data)
            temp_file = f.name

        try:
            # Should handle errors gracefully
            try:
                result = loader.load_deltas(temp_file)
                # If no exception, result should be valid
                assert isinstance(result, list)
            except Exception as e:
                # Exceptions are acceptable for malformed data
                assert isinstance(e, (ValueError | RuntimeError | TypeError))

        finally:
            os.unlink(temp_file)


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
    deltas = loader.load_deltas(filepath, limit=100)

    # Assert
    assert len(deltas) == 15
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
    deltas = loader.load_depth10(filepath, levels=5, limit=100)

    # Assert
    assert len(deltas) == 10
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT.BINANCE")

    # Verify all 10 bid levels (first 5 from data, rest are null/empty)
    assert len(deltas[0].bids) == 10
    expected_bids = [
        ("11657.07", "10.896"),
        ("11656.97", "0.2"),
        ("11655.78", "0.2"),
        ("11655.77", "0.98"),
        ("11655.68", "0.111"),
    ]

    for i, (price, size) in enumerate(expected_bids):
        assert deltas[0].bids[i].price == Price.from_str(price)
        assert deltas[0].bids[i].size == Quantity.from_str(size)
        assert deltas[0].bids[i].side == OrderSide.BUY
        assert deltas[0].bids[i].order_id == 0
    # Levels 5-9 should be empty (price 0, size 0)
    for i in range(5, 10):
        assert deltas[0].bids[i].price == Price.from_int(0)
        assert deltas[0].bids[i].size == Quantity.from_int(0)
        assert deltas[0].bids[i].side == OrderSide.NO_ORDER_SIDE
        assert deltas[0].bids[i].order_id == 0

    # Verify bid prices are strictly decreasing for non-zero levels (logical check)
    for i in range(1, 5):
        assert (
            deltas[0].bids[i].price < deltas[0].bids[i - 1].price  # type: ignore
        ), f"Bid price at level {i} ({deltas[0].bids[i].price}) should be less than level {i-1} ({deltas[0].bids[i-1].price})"

    # Verify all 10 ask levels (first 5 from data, rest are null/empty)
    assert len(deltas[0].asks) == 10
    expected_asks = [
        ("11657.08", "1.714"),
        ("11657.54", "5.4"),
        ("11657.56", "0.238"),
        ("11657.61", "0.077"),
        ("11657.92", "0.918"),
    ]

    for i, (price, size) in enumerate(expected_asks):
        assert deltas[0].asks[i].price == Price.from_str(price)
        assert deltas[0].asks[i].size == Quantity.from_str(size)
        assert deltas[0].asks[i].side == OrderSide.SELL
        assert deltas[0].asks[i].order_id == 0
    # Levels 5-9 should be empty (price 0, size 0)
    for i in range(5, 10):
        assert deltas[0].asks[i].price == Price.from_int(0)
        assert deltas[0].asks[i].size == Quantity.from_int(0)
        assert deltas[0].asks[i].side == OrderSide.NO_ORDER_SIDE
        assert deltas[0].asks[i].order_id == 0

    # Verify ask prices are strictly increasing for non-zero levels (logical check)
    for i in range(1, 5):
        assert (
            deltas[0].asks[i].price > deltas[0].asks[i - 1].price  # type: ignore
        ), f"Ask price at level {i} ({deltas[0].asks[i].price}) should be greater than level {i-1} ({deltas[0].asks[i-1].price})"

    # Verify bid/ask spread is positive (best ask > best bid)
    assert (
        deltas[0].asks[0].price > deltas[0].bids[0].price  # type: ignore
    ), f"Best ask ({deltas[0].asks[0].price}) should be greater than best bid ({deltas[0].bids[0].price})"

    # Verify bid and ask counts
    assert deltas[0].bid_counts[0] == 1
    assert deltas[0].bid_counts[1] == 1
    assert deltas[0].bid_counts[2] == 1
    assert deltas[0].bid_counts[3] == 1
    assert deltas[0].bid_counts[4] == 1
    for i in range(5, 10):
        assert deltas[0].bid_counts[i] == 0

    assert deltas[0].ask_counts[0] == 1
    assert deltas[0].ask_counts[1] == 1
    assert deltas[0].ask_counts[2] == 1
    assert deltas[0].ask_counts[3] == 1
    assert deltas[0].ask_counts[4] == 1
    for i in range(5, 10):
        assert deltas[0].ask_counts[i] == 0

    # Verify metadata
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
    deltas = loader.load_depth10(filepath, levels=25, limit=100)

    # Assert
    assert len(deltas) == 10
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")

    # Verify all 10 bid levels from snapshot25 (only first 10 of 25 are used)
    assert len(deltas[0].bids) == 10
    expected_bids = [
        ("11657.07", "10.896"),
        ("11656.97", "0.2"),
        ("11655.78", "0.2"),
        ("11655.77", "0.98"),
        ("11655.68", "0.111"),
        ("11655.66", "0.077"),
        ("11655.57", "0.34"),
        ("11655.48", "0.4"),
        ("11655.26", "1.185"),
        ("11654.86", "0.195"),
    ]

    for i, (price, size) in enumerate(expected_bids):
        assert deltas[0].bids[i].price == Price.from_str(price)
        assert deltas[0].bids[i].size == Quantity.from_str(size)
        assert deltas[0].bids[i].side == OrderSide.BUY
        assert deltas[0].bids[i].order_id == 0

    # Verify bid prices are strictly decreasing (logical check)
    for i in range(1, 10):
        assert (
            deltas[0].bids[i].price < deltas[0].bids[i - 1].price  # type: ignore
        ), f"Bid price at level {i} ({deltas[0].bids[i].price}) should be less than level {i-1} ({deltas[0].bids[i-1].price})"

    # Verify all 10 ask levels from snapshot25 (only first 10 of 25 are used)
    assert len(deltas[0].asks) == 10
    expected_asks = [
        ("11657.08", "1.714"),
        ("11657.54", "5.4"),
        ("11657.56", "0.238"),
        ("11657.61", "0.077"),
        ("11657.92", "0.918"),
        ("11658.09", "1.015"),
        ("11658.12", "0.665"),
        ("11658.19", "0.583"),
        ("11658.28", "0.255"),
        ("11658.29", "0.656"),
    ]

    for i, (price, size) in enumerate(expected_asks):
        assert deltas[0].asks[i].price == Price.from_str(price)
        assert deltas[0].asks[i].size == Quantity.from_str(size)
        assert deltas[0].asks[i].side == OrderSide.SELL
        assert deltas[0].asks[i].order_id == 0

    # Verify ask prices are strictly increasing (logical check)
    for i in range(1, 10):
        assert (
            deltas[0].asks[i].price > deltas[0].asks[i - 1].price  # type: ignore
        ), f"Ask price at level {i} ({deltas[0].asks[i].price}) should be greater than level {i-1} ({deltas[0].asks[i-1].price})"

    # Verify bid/ask spread is positive (best ask > best bid)
    assert (
        deltas[0].asks[0].price > deltas[0].bids[0].price  # type: ignore
    ), f"Best ask ({deltas[0].asks[0].price}) should be greater than best bid ({deltas[0].bids[0].price})"

    # Verify bid and ask counts (all should be 1 for snapshot data)
    for i in range(10):
        assert deltas[0].bid_counts[i] == 1
        assert deltas[0].ask_counts[i] == 1

    # Verify metadata
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
    trades = loader.load_quotes(filepath, limit=100)

    # Assert
    assert len(trades) == 10
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
    trades = loader.load_trades(filepath, limit=100)

    # Assert
    assert len(trades) == 10
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
            f"Dynamic precision test: {len(deltas)} deltas in {elapsed_time:.3f}s, {memory_used:.2f} MB",
        )
    finally:
        os.unlink(temp_file)


def test_tardis_stream_deltas():
    """
    Test async streaming functionality for order book deltas.
    """
    # Create synthetic CSV data with varying precision
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance-futures,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance-futures,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.5,2.0
binance-futures,BTCUSDT,1640995202000000,1640995202100000,false,ask,50000.12,1.5
binance-futures,BTCUSDT,1640995203000000,1640995203100000,false,bid,49999.123,3.0
binance-futures,BTCUSDT,1640995204000000,1640995204100000,false,ask,50000.1234,0.5
binance-futures,BTCUSDT,1640995205000000,1640995205100000,false,bid,49998.5,1.0"""

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
            instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
        )

        # Test streaming with chunk size of 2
        chunks = []
        chunk_count = 0

        for chunk in loader.stream_deltas(temp_file, chunk_size=2):
            chunks.append(chunk)
            chunk_count += 1

            # Verify chunk properties
            assert isinstance(chunk, list)
            assert len(chunk) <= 2  # Should not exceed chunk size
            assert len(chunk) > 0  # Should not be empty

            # Verify delta properties
            for delta in chunk:
                assert delta.instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
                assert delta.action in [BookAction.ADD, BookAction.UPDATE, BookAction.DELETE]
                assert delta.order.side in [OrderSide.BUY, OrderSide.SELL]
                assert delta.order.price is not None
                assert delta.order.size is not None

        # Verify chunking worked correctly
        assert chunk_count == 3  # 6 records / 2 per chunk = 3 chunks
        assert len(chunks[0]) == 2  # First chunk: 2 records
        assert len(chunks[1]) == 2  # Second chunk: 2 records
        assert len(chunks[2]) == 2  # Third chunk: 2 records

        # Verify total records
        total_deltas = sum(len(chunk) for chunk in chunks)
        assert total_deltas == 6

        # Test precision inference within chunks
        # Each chunk should have consistent precision within itself
        for i, chunk in enumerate(chunks):
            if len(chunk) > 1:
                # All deltas in a chunk should have same precision
                first_price_precision = chunk[0].order.price.precision
                first_size_precision = chunk[0].order.size.precision

                for delta in chunk[1:]:
                    assert delta.order.price.precision == first_price_precision
                    assert delta.order.size.precision == first_size_precision

        # Test streaming with different chunk size
        chunks_large = []
        for chunk in loader.stream_deltas(temp_file, chunk_size=4):
            chunks_large.append(chunk)

        # Should have 2 chunks: [4 items, 2 items]
        assert len(chunks_large) == 2
        assert len(chunks_large[0]) == 4
        assert len(chunks_large[1]) == 2

        print(f"Streaming test: {chunk_count} chunks processed, {total_deltas} total deltas")
    finally:
        os.unlink(temp_file)


def test_tardis_stream_deltas_memory_efficient():
    """
    Test that streaming is memory efficient compared to loading all at once.
    """
    # Create larger synthetic dataset
    csv_rows = ["exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount"]

    # Generate 1000 records
    for i in range(1000):
        timestamp = 1640995200000000 + i * 1000
        local_timestamp = timestamp + 100000
        side = "ask" if i % 2 == 0 else "bid"
        price = 50000.0 + (i % 100) * 0.01
        amount = 1.0 + (i % 50) * 0.1

        csv_rows.append(
            f"binance-futures,BTCUSDT,{timestamp},{local_timestamp},false,{side},{price},{amount}",
        )

    csv_data = "\n".join(csv_rows)

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test that we can process large file in small chunks
        process = psutil.Process(os.getpid())
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB

        chunk_count = 0
        total_processed = 0

        # Process in very small chunks to test memory efficiency
        for chunk in loader.stream_deltas(temp_file, chunk_size=50):
            chunk_count += 1
            total_processed += len(chunk)

            # Memory check - should stay relatively stable
            current_memory = process.memory_info().rss / 1024 / 1024
            memory_increase = current_memory - initial_memory

            # Should not use excessive memory even with 1000 records
            assert memory_increase < 100, f"Memory usage too high: {memory_increase:.2f} MB"

        assert total_processed == 1000
        assert chunk_count == 20  # 1000 / 50 = 20 chunks

        print(f"Memory efficiency test: {total_processed} records in {chunk_count} chunks")
    finally:
        os.unlink(temp_file)


def test_tardis_stream_quotes():
    """
    Test streaming functionality for quote ticks.
    """
    # Create synthetic CSV data for quotes
    csv_data = """exchange,symbol,timestamp,local_timestamp,bid_price,bid_size,ask_price,ask_size
huobi,BTC-USD,1588291201099000,1588291201234268,8629.1,800,8629.3,5000
huobi,BTC-USD,1588291202099000,1588291202234268,8629.2,806,8629.4,5494
huobi,BTC-USD,1588291203099000,1588291203234268,8629.15,850,8629.35,5200
huobi,BTC-USD,1588291204099000,1588291204234268,8629.25,900,8629.45,5800"""

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
            instrument_id=InstrumentId.from_str("BTC-USD.HUOBI"),
        )

        # Test streaming with chunk size of 2
        chunks = []
        chunk_count = 0

        for chunk in loader.stream_quotes(temp_file, chunk_size=2):
            chunks.append(chunk)
            chunk_count += 1

            # Verify chunk properties
            assert isinstance(chunk, list)
            assert len(chunk) <= 2  # Should not exceed chunk size
            assert len(chunk) > 0  # Should not be empty

            # Verify quote properties
            for quote in chunk:
                assert quote.instrument_id == InstrumentId.from_str("BTC-USD.HUOBI")
                assert quote.bid_price is not None
                assert quote.ask_price is not None
                assert quote.bid_size is not None
                assert quote.ask_size is not None

        # Verify chunking worked correctly
        assert chunk_count == 2  # 4 records / 2 per chunk = 2 chunks
        assert len(chunks[0]) == 2  # First chunk: 2 records
        assert len(chunks[1]) == 2  # Second chunk: 2 records

        # Verify total records
        total_quotes = sum(len(chunk) for chunk in chunks)
        assert total_quotes == 4

        print(
            f"Quote streaming test: {chunk_count} chunks processed, {total_quotes} total quotes",
        )
    finally:
        os.unlink(temp_file)


def test_tardis_stream_trades():
    """
    Test streaming functionality for trade ticks.
    """
    # Create synthetic CSV data for trades
    csv_data = """exchange,symbol,timestamp,local_timestamp,id,side,price,amount
bitmex,XBTUSD,1583020803145000,1583020803307160,ccc3c1fa-212c-e8b0-1706-9b9c4f3d5ecf,sell,8531.5,2152
bitmex,XBTUSD,1583020804145000,1583020804307160,ddd4d2fb-313d-f9c1-2807-aca5d4e6d6d0,buy,8532.0,1500
bitmex,XBTUSD,1583020805145000,1583020805307160,eee5e3fc-424e-0ad2-3918-bdb6e5f7e7e1,sell,8531.8,3000
bitmex,XBTUSD,1583020806145000,1583020806307160,fff6f4fd-535f-1be3-4a29-cec7f6080802,buy,8532.2,1800"""

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
            instrument_id=InstrumentId.from_str("XBTUSD.BITMEX"),
        )

        # Test streaming with chunk size of 3
        chunks = []
        chunk_count = 0

        for chunk in loader.stream_trades(temp_file, chunk_size=3):
            chunks.append(chunk)
            chunk_count += 1

            # Verify chunk properties
            assert isinstance(chunk, list)
            assert len(chunk) <= 3  # Should not exceed chunk size
            assert len(chunk) > 0  # Should not be empty

            # Verify trade properties
            for trade in chunk:
                assert trade.instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
                assert trade.price is not None
                assert trade.size is not None
                assert trade.aggressor_side is not None
                assert trade.trade_id is not None

        # Verify chunking worked correctly
        assert chunk_count == 2  # 4 records / 3 per chunk = 2 chunks (3, 1)
        assert len(chunks[0]) == 3  # First chunk: 3 records
        assert len(chunks[1]) == 1  # Second chunk: 1 record

        # Verify total records
        total_trades = sum(len(chunk) for chunk in chunks)
        assert total_trades == 4

        print(
            f"Trade streaming test: {chunk_count} chunks processed, {total_trades} total trades",
        )
    finally:
        os.unlink(temp_file)


def test_tardis_stream_depth10_snapshot5():
    """
    Test streaming functionality for order book depth10 from snapshot5.
    """
    # Create synthetic CSV data for snapshot5
    header = (
        "exchange,symbol,timestamp,local_timestamp,"
        "bids[0].price,bids[0].amount,bids[1].price,bids[1].amount,"
        "bids[2].price,bids[2].amount,bids[3].price,bids[3].amount,"
        "bids[4].price,bids[4].amount,asks[0].price,asks[0].amount,"
        "asks[1].price,asks[1].amount,asks[2].price,asks[2].amount,"
        "asks[3].price,asks[3].amount,asks[4].price,asks[4].amount"
    )
    row1 = (
        "binance,BTCUSDT,1598918403696000,1598918403810979,"
        "11657.07,10.896,11657.06,5.432,11657.05,8.123,11657.04,12.567,"
        "11657.03,15.234,11657.08,1.714,11657.09,3.456,11657.10,7.890,"
        "11657.11,9.123,11657.12,11.567"
    )
    row2 = (
        "binance,BTCUSDT,1598918404696000,1598918404810979,"
        "11657.08,11.000,11657.07,5.500,11657.06,8.200,11657.05,12.600,"
        "11657.04,15.300,11657.09,1.800,11657.10,3.500,11657.11,8.000,"
        "11657.12,9.200,11657.13,11.600"
    )
    csv_data = f"{header}\n{row1}\n{row2}"

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
            instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
        )

        # Test streaming with chunk size of 1
        chunks = []
        chunk_count = 0

        for chunk in loader.stream_depth10(temp_file, levels=5, chunk_size=1):
            chunks.append(chunk)
            chunk_count += 1

            # Verify chunk properties
            assert isinstance(chunk, list)
            assert len(chunk) <= 1  # Should not exceed chunk size
            assert len(chunk) > 0  # Should not be empty

            # Verify depth properties
            for depth in chunk:
                assert depth.instrument_id == InstrumentId.from_str("BTCUSDT.BINANCE")
                assert len(depth.bids) == 10  # Always 10 levels
                assert len(depth.asks) == 10  # Always 10 levels
                # First 5 levels should have real data, others should be null orders
                for i in range(5):
                    assert depth.bids[i].price is not None
                    assert depth.asks[i].price is not None

        # Verify chunking worked correctly
        assert chunk_count == 2  # 2 records / 1 per chunk = 2 chunks
        assert len(chunks[0]) == 1  # First chunk: 1 record
        assert len(chunks[1]) == 1  # Second chunk: 1 record

        # Verify total records
        total_depths = sum(len(chunk) for chunk in chunks)
        assert total_depths == 2

        print(
            f"Depth10 snapshot5 streaming test: {chunk_count} chunks processed, {total_depths} total depths",
        )
    finally:
        os.unlink(temp_file)


def _generate_quotes_csv(num_records):
    """
    Generate synthetic CSV data for quotes.
    """
    rows = ["exchange,symbol,timestamp,local_timestamp,bid_price,bid_size,ask_price,ask_size"]
    for i in range(num_records):
        timestamp = 1588291201099000 + i * 1000
        local_timestamp = timestamp + 100000
        bid_price = 8629.0 + (i % 100) * 0.01
        ask_price = bid_price + 0.1
        bid_size = 800 + (i % 50) * 10
        ask_size = 5000 + (i % 50) * 100
        rows.append(
            f"huobi,BTC-USD,{timestamp},{local_timestamp},{bid_price},{bid_size},{ask_price},{ask_size}",
        )
    return "\n".join(rows)


def _generate_trades_csv(num_records):
    """
    Generate synthetic CSV data for trades.
    """
    rows = ["exchange,symbol,timestamp,local_timestamp,id,side,price,amount"]
    for i in range(num_records):
        timestamp = 1583020803145000 + i * 1000
        local_timestamp = timestamp + 100000
        price = 8531.0 + (i % 100) * 0.01
        amount = 1500 + (i % 50) * 100
        side = "buy" if i % 2 == 0 else "sell"
        trade_id = f"{i:08x}-{i:04x}-{i:04x}-{i:04x}-{i:012x}"
        rows.append(
            f"bitmex,XBTUSD,{timestamp},{local_timestamp},{trade_id},{side},{price},{amount}",
        )
    return "\n".join(rows)


def _generate_deltas_csv(num_records):
    """
    Generate synthetic CSV data for deltas.
    """
    rows = ["exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount"]
    for i in range(num_records):
        timestamp = 1640995200000000 + i * 1000
        local_timestamp = timestamp + 100000
        is_snapshot = "true" if i == 0 else "false"
        side = "ask" if i % 2 == 0 else "bid"
        price = 50000.0 + (i % 100) * 0.01
        amount = 1.0 + (i % 50) * 0.1
        rows.append(
            f"binance-futures,BTCUSDT,{timestamp},{local_timestamp},{is_snapshot},{side},{price},{amount}",
        )
    return "\n".join(rows)


def _generate_depth10_snapshot5_csv(num_records):
    """
    Generate synthetic CSV data for depth10 snapshot5.
    """
    header = (
        "exchange,symbol,timestamp,local_timestamp,"
        "bids[0].price,bids[0].amount,bids[1].price,bids[1].amount,"
        "bids[2].price,bids[2].amount,bids[3].price,bids[3].amount,"
        "bids[4].price,bids[4].amount,asks[0].price,asks[0].amount,"
        "asks[1].price,asks[1].amount,asks[2].price,asks[2].amount,"
        "asks[3].price,asks[3].amount,asks[4].price,asks[4].amount"
    )
    rows = [header]

    for i in range(num_records):
        timestamp = 1598918403696000 + i * 1000
        local_timestamp = timestamp + 100000
        base_bid_price = 11657.0 - (i % 10) * 0.01
        base_ask_price = base_bid_price + 0.01

        # Generate 5 bid levels and 5 ask levels
        bid_data = []
        ask_data = []

        for level in range(5):
            bid_price = base_bid_price - level * 0.01
            bid_amount = 10.0 + (i + level) % 20
            ask_price = base_ask_price + level * 0.01
            ask_amount = 10.0 + (i + level + 5) % 20

            bid_data.extend([f"{bid_price:.2f}", f"{bid_amount:.3f}"])
            ask_data.extend([f"{ask_price:.2f}", f"{ask_amount:.3f}"])

        row = f"binance,BTCUSDT,{timestamp},{local_timestamp}," + ",".join(bid_data + ask_data)
        rows.append(row)

    return "\n".join(rows)


def _generate_depth10_snapshot25_csv(num_records):
    """
    Generate synthetic CSV data for depth10 snapshot25.
    """
    # Snapshot25 has 25 levels but we only need the first 10 for depth10
    header_parts = ["exchange,symbol,timestamp,local_timestamp"]

    # Add bid levels
    for i in range(25):
        header_parts.extend([f"bids[{i}].price", f"bids[{i}].amount"])

    # Add ask levels
    for i in range(25):
        header_parts.extend([f"asks[{i}].price", f"asks[{i}].amount"])

    header = ",".join(header_parts)
    rows = [header]

    for i in range(num_records):
        timestamp = 1598918403696000 + i * 1000
        local_timestamp = timestamp + 100000
        base_bid_price = 11657.0 - (i % 10) * 0.01
        base_ask_price = base_bid_price + 0.01

        # Generate 25 bid levels and 25 ask levels
        data_parts = [f"binance,BTCUSDT,{timestamp},{local_timestamp}"]

        # Bids
        for level in range(25):
            bid_price = base_bid_price - level * 0.01
            bid_amount = 10.0 + (i + level) % 20
            data_parts.extend([f"{bid_price:.2f}", f"{bid_amount:.3f}"])

        # Asks
        for level in range(25):
            ask_price = base_ask_price + level * 0.01
            ask_amount = 10.0 + (i + level + 25) % 20
            data_parts.extend([f"{ask_price:.2f}", f"{ask_amount:.3f}"])

        rows.append(",".join(data_parts))

    return "\n".join(rows)


def _test_memory_efficiency_for_type(data_type, csv_generator):
    """
    Test memory efficiency for a specific data type.
    """
    csv_data = csv_generator(500)  # Generate 500 records

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()
        process = psutil.Process(os.getpid())
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB

        chunk_count = 0
        total_processed = 0

        # Get the appropriate streaming method
        if data_type == "quotes":
            stream_iter = loader.stream_quotes(temp_file, chunk_size=50)
        elif data_type == "trades":
            stream_iter = loader.stream_trades(temp_file, chunk_size=50)
        elif data_type == "depth10_snapshot5":
            stream_iter = loader.stream_depth10(temp_file, levels=5, chunk_size=50)
        elif data_type == "depth10_snapshot25":
            stream_iter = loader.stream_depth10(temp_file, levels=25, chunk_size=50)
        else:  # deltas
            stream_iter = loader.stream_deltas(temp_file, chunk_size=50)

        for chunk in stream_iter:
            chunk_count += 1
            total_processed += len(chunk)

            # Memory check - should stay relatively stable
            current_memory = process.memory_info().rss / 1024 / 1024
            memory_increase = current_memory - initial_memory

            # Should not use excessive memory even with 500 records
            assert (
                memory_increase < 150
            ), f"Memory usage too high for {data_type}: {memory_increase:.2f} MB"

        assert total_processed == 500
        assert chunk_count == 10  # 500 / 50 = 10

        print(
            f"{data_type.capitalize()} memory efficiency: {total_processed} records in {chunk_count} chunks",
        )
    finally:
        os.unlink(temp_file)


def test_tardis_stream_memory_efficiency_all_types():
    test_cases = [
        ("deltas", _generate_deltas_csv),
        ("quotes", _generate_quotes_csv),
        ("trades", _generate_trades_csv),
        ("depth10_snapshot5", _generate_depth10_snapshot5_csv),
        ("depth10_snapshot25", _generate_depth10_snapshot25_csv),
    ]

    for data_type, csv_generator in test_cases:
        _test_memory_efficiency_for_type(data_type, csv_generator)


def test_tardis_load_trades_from_stub_data():
    # Arrange
    filepath = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    trades = loader.load_trades(filepath)

    # Assert
    assert len(trades) == 2
    assert trades[0].price == Price.from_str("8531.5")
    assert trades[1].size == Quantity.from_int(1000)


def test_tardis_load_deltas_from_stub_data():
    # Arrange
    filepath = get_test_data_path("deltas_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    deltas = loader.load_deltas(filepath)

    # Assert
    assert len(deltas) == 2
    assert deltas[0].order.price == Price.from_str("6421.5")
    assert deltas[1].order.size == Quantity.from_int(10000)


def test_tardis_stream_trades_from_stub_data():
    # Arrange
    filepath = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    stream = loader.stream_trades(filepath, chunk_size=1)
    chunks = list(stream)

    # Assert
    assert len(chunks) == 2
    assert len(chunks[0]) == 1
    assert chunks[0][0].price == Price.from_str("8531.5")
    assert len(chunks[1]) == 1
    assert chunks[1][0].size == Quantity.from_int(1000)


def test_tardis_stream_deltas_from_stub_data():
    # Arrange
    filepath = get_test_data_path("deltas_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    stream = loader.stream_deltas(filepath, chunk_size=1)
    chunks = list(stream)

    # Assert
    assert len(chunks) == 2
    assert len(chunks[0]) == 1
    assert chunks[0][0].order.price == Price.from_str("6421.5")
    assert len(chunks[1]) == 1
    assert chunks[1][0].order.size == Quantity.from_int(10000)


def test_precision_inference_with_minimal_data():
    # Arrange
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,true,ask,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,false,bid,49999.12,2.00"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader(
            price_precision=None,  # Infer
            size_precision=2,
        )

        # Act
        deltas = loader.load_deltas(temp_file)

        # Assert
        assert len(deltas) == 2

        for delta in deltas:
            assert delta.order.price.precision == 2
            assert delta.order.size.precision == 2
    finally:
        os.unlink(temp_file)


def test_price_and_size_precision_from_stub_data():
    # Arrange
    filepath = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    trades = loader.load_trades(filepath)

    # Assert
    for trade in trades:
        assert trade.price.precision == 1
        assert trade.size.precision == 0


def test_inferred_price_and_size_precision_from_stub_data():
    # Arrange
    filepath = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader(price_precision=None, size_precision=None)

    # Act
    trades = loader.load_trades(filepath)

    # Assert
    for trade in trades:
        assert trade.price.precision == 1
        assert trade.size.precision == 0


def test_deltas_specific_price_size_values_from_stub_data():
    # Arrange
    deltas_path = get_test_data_path("deltas_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    deltas = loader.load_deltas(deltas_path)

    # Assert
    assert deltas[0].order.price == Price.from_str("6421.5")
    assert deltas[0].order.size == Quantity.from_str("18640")
    assert deltas[1].order.price == Price.from_str("6421.0")
    assert deltas[1].order.size == Quantity.from_str("10000")


def test_trades_specific_price_size_values_from_stub_data():
    # Arrange
    trades_path = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader(price_precision=1, size_precision=0)

    # Act
    trades = loader.load_trades(trades_path)

    # Assert
    assert trades[0].price == Price.from_str("8531.5")
    assert trades[0].size == Quantity.from_str("2152")
    assert trades[1].price == Price.from_str("8531.0")
    assert trades[1].size == Quantity.from_str("1000")


def test_tardis_load_deltas_with_limit():
    """
    Test load_deltas with limit parameter.
    """
    # Create synthetic CSV data with 10 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,false,bid,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,false,ask,50001.0,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,false,bid,49999.0,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,false,ask,50002.0,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,false,bid,49998.0,0.5
binance,BTCUSDT,1640995205000000,1640995205100000,false,ask,50003.0,2.5
binance,BTCUSDT,1640995206000000,1640995206100000,false,bid,49997.0,1.2
binance,BTCUSDT,1640995207000000,1640995207100000,false,ask,50004.0,3.5
binance,BTCUSDT,1640995208000000,1640995208100000,false,bid,49996.0,0.8
binance,BTCUSDT,1640995209000000,1640995209100000,false,ask,50005.0,2.8"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test limit of 5 records
        deltas = loader.load_deltas(temp_file, limit=5)
        assert len(deltas) == 5

        # Verify we get the first 5 records in order
        assert deltas[0].order.price == Price.from_str("50000.0")
        assert deltas[1].order.price == Price.from_str("50001.0")
        assert deltas[2].order.price == Price.from_str("49999.0")
        assert deltas[3].order.price == Price.from_str("50002.0")
        assert deltas[4].order.price == Price.from_str("49998.0")

        # Test limit larger than available records
        deltas_all = loader.load_deltas(temp_file, limit=20)
        assert len(deltas_all) == 10  # Should return all 10 records

        # Test limit of 1
        deltas_one = loader.load_deltas(temp_file, limit=1)
        assert len(deltas_one) == 1
        assert deltas_one[0].order.price == Price.from_str("50000.0")

    finally:
        os.unlink(temp_file)


def test_tardis_load_quotes_with_limit():
    """
    Test load_quotes with limit parameter.
    """
    # Create synthetic CSV data with 8 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,ask_amount,ask_price,bid_price,bid_amount
huobi,BTC-USD,1588291201099000,1588291201234268,5000,8629.3,8629.1,800
huobi,BTC-USD,1588291202099000,1588291202234268,5494,8629.4,8629.2,806
huobi,BTC-USD,1588291203099000,1588291203234268,5200,8629.35,8629.15,850
huobi,BTC-USD,1588291204099000,1588291204234268,5800,8629.45,8629.25,900
huobi,BTC-USD,1588291205099000,1588291205234268,5100,8629.5,8629.3,820
huobi,BTC-USD,1588291206099000,1588291206234268,5600,8629.55,8629.35,880
huobi,BTC-USD,1588291207099000,1588291207234268,5300,8629.6,8629.4,840
huobi,BTC-USD,1588291208099000,1588291208234268,5700,8629.65,8629.45,860"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test limit of 3 records
        quotes = loader.load_quotes(temp_file, limit=3)
        assert len(quotes) == 3

        # Verify we get the first 3 records in order
        assert quotes[0].bid_price == Price.from_str("8629.1")
        assert quotes[0].ask_price == Price.from_str("8629.3")
        assert quotes[1].bid_price == Price.from_str("8629.2")
        assert quotes[1].ask_price == Price.from_str("8629.4")
        assert quotes[2].bid_price == Price.from_str("8629.15")
        assert quotes[2].ask_price == Price.from_str("8629.35")

    finally:
        os.unlink(temp_file)


def test_tardis_load_trades_with_limit():
    """
    Test load_trades with limit parameter.
    """
    # Create synthetic CSV data with 6 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,id,side,price,amount
bitmex,XBTUSD,1583020803145000,1583020803307160,trade1,sell,8531.5,2152
bitmex,XBTUSD,1583020804145000,1583020804307160,trade2,buy,8532.0,1500
bitmex,XBTUSD,1583020805145000,1583020805307160,trade3,sell,8531.8,3000
bitmex,XBTUSD,1583020806145000,1583020806307160,trade4,buy,8532.2,1800
bitmex,XBTUSD,1583020807145000,1583020807307160,trade5,sell,8531.9,2500
bitmex,XBTUSD,1583020808145000,1583020808307160,trade6,buy,8532.1,1900"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test limit of 4 records
        trades = loader.load_trades(temp_file, limit=4)
        assert len(trades) == 4

        # Verify we get the first 4 records in order
        assert trades[0].price == Price.from_str("8531.5")
        assert trades[0].trade_id == TradeId("trade1")
        assert trades[1].price == Price.from_str("8532.0")
        assert trades[1].trade_id == TradeId("trade2")
        assert trades[2].price == Price.from_str("8531.8")
        assert trades[2].trade_id == TradeId("trade3")
        assert trades[3].price == Price.from_str("8532.2")
        assert trades[3].trade_id == TradeId("trade4")

    finally:
        os.unlink(temp_file)


def test_tardis_load_depth10_with_limit():
    """
    Test load_depth10 with limit parameter for both snapshot5 and snapshot25.
    """
    # Create synthetic CSV data for snapshot5 with 4 records
    snapshot5_header = (
        "exchange,symbol,timestamp,local_timestamp,"
        "bids[0].price,bids[0].amount,bids[1].price,bids[1].amount,"
        "bids[2].price,bids[2].amount,bids[3].price,bids[3].amount,"
        "bids[4].price,bids[4].amount,asks[0].price,asks[0].amount,"
        "asks[1].price,asks[1].amount,asks[2].price,asks[2].amount,"
        "asks[3].price,asks[3].amount,asks[4].price,asks[4].amount"
    )
    snapshot5_rows = [
        "binance,BTCUSDT,1598918403696000,1598918403810979,11657.07,10.896,11657.06,5.432,11657.05,8.123,11657.04,12.567,11657.03,15.234,11657.08,1.714,11657.09,3.456,11657.10,7.890,11657.11,9.123,11657.12,11.567",
        "binance,BTCUSDT,1598918404696000,1598918404810979,11658.07,11.000,11658.06,5.500,11658.05,8.200,11658.04,12.600,11658.03,15.300,11658.08,1.800,11658.09,3.500,11658.10,8.000,11658.11,9.200,11658.12,11.600",
        "binance,BTCUSDT,1598918405696000,1598918405810979,11659.07,10.500,11659.06,5.100,11659.05,8.300,11659.04,12.200,11659.03,15.100,11659.08,1.900,11659.09,3.200,11659.10,8.100,11659.11,9.300,11659.12,11.200",
        "binance,BTCUSDT,1598918406696000,1598918406810979,11660.07,10.200,11660.06,5.800,11660.05,8.400,11660.04,12.100,11660.03,15.500,11660.08,1.600,11660.09,3.800,11660.10,8.200,11660.11,9.400,11660.12,11.800",
    ]
    snapshot5_csv = f"{snapshot5_header}\n" + "\n".join(snapshot5_rows)

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(snapshot5_csv)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test limit of 2 records for snapshot5
        depths = loader.load_depth10(temp_file, levels=5, limit=2)
        assert len(depths) == 2

        # Verify we get the first 2 records
        assert depths[0].bids[0].price == Price.from_str("11657.06")
        assert depths[0].asks[0].price == Price.from_str("11657.07")
        assert depths[1].bids[0].price == Price.from_str("11658.06")
        assert depths[1].asks[0].price == Price.from_str("11658.07")

        # Each depth should have 10 bid and ask levels
        for depth in depths:
            assert len(depth.bids) == 10
            assert len(depth.asks) == 10

    finally:
        os.unlink(temp_file)


def test_tardis_stream_deltas_with_limit():
    """
    Test stream_deltas with limit parameter.
    """
    # Create synthetic CSV data with 12 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,false,bid,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,false,ask,50001.0,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,false,bid,49999.0,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,false,ask,50002.0,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,false,bid,49998.0,0.5
binance,BTCUSDT,1640995205000000,1640995205100000,false,ask,50003.0,2.5
binance,BTCUSDT,1640995206000000,1640995206100000,false,bid,49997.0,1.2
binance,BTCUSDT,1640995207000000,1640995207100000,false,ask,50004.0,3.5
binance,BTCUSDT,1640995208000000,1640995208100000,false,bid,49996.0,0.8
binance,BTCUSDT,1640995209000000,1640995209100000,false,ask,50005.0,2.8
binance,BTCUSDT,1640995210000000,1640995210100000,false,bid,49995.0,1.8
binance,BTCUSDT,1640995211000000,1640995211100000,false,ask,50006.0,3.8"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test streaming with limit of 7 records, chunk size of 3
        limited_chunks = list(loader.stream_deltas(temp_file, chunk_size=3, limit=7))
        total_limited = sum(len(chunk) for chunk in limited_chunks)

        # Should only process 7 records due to limit
        assert total_limited == 7

        # Should have 3 chunks: [3, 3, 1] = 7 records total (limited)
        assert len(limited_chunks) == 3
        assert len(limited_chunks[0]) == 3
        assert len(limited_chunks[1]) == 3
        assert len(limited_chunks[2]) == 1

        # Test without limit should process all 12 records
        all_chunks = list(loader.stream_deltas(temp_file, chunk_size=3))
        total_from_streaming = sum(len(chunk) for chunk in all_chunks)

        # Should be 4 chunks: [3, 3, 3, 3] = 12 records total
        assert len(all_chunks) == 4
        assert total_from_streaming == 12

        # Verify the limited data contains the first 7 records
        all_limited_deltas = []
        for chunk in limited_chunks:
            all_limited_deltas.extend(chunk)

        assert len(all_limited_deltas) == 7
        assert all_limited_deltas[0].order.price == Price.from_str("50000.0")
        assert all_limited_deltas[1].order.price == Price.from_str("50001.0")
        assert all_limited_deltas[6].order.price == Price.from_str("49997.0")  # 7th record

    finally:
        os.unlink(temp_file)


def test_tardis_stream_trades_with_limit():
    """
    Test stream_trades with limit parameter.
    """
    # Create synthetic CSV data with 10 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,id,side,price,amount
bitmex,XBTUSD,1583020803145000,1583020803307160,trade1,sell,8531.5,2152
bitmex,XBTUSD,1583020804145000,1583020804307160,trade2,buy,8532.0,1500
bitmex,XBTUSD,1583020805145000,1583020805307160,trade3,sell,8531.8,3000
bitmex,XBTUSD,1583020806145000,1583020806307160,trade4,buy,8532.2,1800
bitmex,XBTUSD,1583020807145000,1583020807307160,trade5,sell,8531.9,2500
bitmex,XBTUSD,1583020808145000,1583020808307160,trade6,buy,8532.1,1900
bitmex,XBTUSD,1583020809145000,1583020809307160,trade7,sell,8531.7,2200
bitmex,XBTUSD,1583020810145000,1583020810307160,trade8,buy,8532.3,1700
bitmex,XBTUSD,1583020811145000,1583020811307160,trade9,sell,8531.6,2800
bitmex,XBTUSD,1583020812145000,1583020812307160,trade10,buy,8532.4,1600"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test streaming with limit of 6 records, chunk size 4
        limited_chunks = list(loader.stream_trades(temp_file, chunk_size=4, limit=6))
        total_limited = sum(len(chunk) for chunk in limited_chunks)

        # Should only process 6 records due to limit
        assert total_limited == 6

        # Should have 2 chunks: [4, 2] = 6 records total (limited)
        assert len(limited_chunks) == 2
        assert len(limited_chunks[0]) == 4
        assert len(limited_chunks[1]) == 2

        # Test without limit should get all 10 records
        all_chunks = list(loader.stream_trades(temp_file, chunk_size=4))
        total_from_streaming = sum(len(chunk) for chunk in all_chunks)

        assert len(all_chunks) == 3
        assert len(all_chunks[0]) == 4
        assert len(all_chunks[1]) == 4
        assert len(all_chunks[2]) == 2
        assert total_from_streaming == 10

        # Verify the limited data contains the first 6 records
        all_limited_trades = []
        for chunk in limited_chunks:
            all_limited_trades.extend(chunk)

        assert len(all_limited_trades) == 6
        assert all_limited_trades[0].trade_id == TradeId("trade1")
        assert all_limited_trades[1].trade_id == TradeId("trade2")
        assert all_limited_trades[5].trade_id == TradeId("trade6")  # 6th record

    finally:
        os.unlink(temp_file)


def test_tardis_stream_batched_deltas_with_limit():
    """
    Test stream_batched_deltas with limit parameter.
    """
    # Create synthetic CSV data with 8 records
    csv_data = """exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
binance,BTCUSDT,1640995200000000,1640995200100000,false,bid,50000.0,1.0
binance,BTCUSDT,1640995201000000,1640995201100000,false,ask,50001.0,2.0
binance,BTCUSDT,1640995202000000,1640995202100000,false,bid,49999.0,1.5
binance,BTCUSDT,1640995203000000,1640995203100000,false,ask,50002.0,3.0
binance,BTCUSDT,1640995204000000,1640995204100000,false,bid,49998.0,0.5
binance,BTCUSDT,1640995205000000,1640995205100000,false,ask,50003.0,2.5
binance,BTCUSDT,1640995206000000,1640995206100000,false,bid,49997.0,1.2
binance,BTCUSDT,1640995207000000,1640995207100000,false,ask,50004.0,3.5"""

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test streaming batched deltas with chunk size 3 and limit 5
        all_chunks = list(loader.stream_batched_deltas(temp_file, chunk_size=3, limit=5))

        # Should get chunks based on the limit in the Rust implementation
        # This test verifies the parameter is accepted and passed through
        assert len(all_chunks) >= 1  # Should get at least one chunk

        # Test without limit
        all_chunks_no_limit = list(loader.stream_batched_deltas(temp_file, chunk_size=3))
        total_no_limit = sum(len(chunk) for chunk in all_chunks_no_limit)

        # Should process all 8 records when no limit
        # Chunks: [3, 3, 2] = 8 total
        assert len(all_chunks_no_limit) == 3
        assert total_no_limit == 8

    finally:
        os.unlink(temp_file)


def test_tardis_load_funding_rates_from_stub_data():
    """
    Test loading funding rates from test CSV data.
    """
    # Arrange
    filepath = get_test_data_path("derivative_ticker_1.csv")
    loader = TardisCSVDataLoader()

    # Act
    funding_rates = loader.load_funding_rates(filepath)

    # Assert
    assert len(funding_rates) == 2
    assert isinstance(funding_rates[0], FundingRateUpdate)
    assert funding_rates[0].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert funding_rates[0].rate == Decimal("0.0001")
    assert funding_rates[0].ts_event == 1583020803145000000
    assert funding_rates[0].ts_init == 1583020803307160000

    assert funding_rates[1].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert funding_rates[1].rate == Decimal("0.00015")
    assert funding_rates[1].ts_event == 1583020863145000000
    assert funding_rates[1].ts_init == 1583020863307160000


def test_tardis_stream_funding_rates_from_stub_data():
    """
    Test streaming funding rates from test CSV data.
    """
    # Arrange
    filepath = get_test_data_path("derivative_ticker_1.csv")
    loader = TardisCSVDataLoader()

    # Act
    stream = loader.stream_funding_rates(filepath, chunk_size=1)
    chunks = list(stream)

    # Assert
    assert len(chunks) == 2
    assert len(chunks[0]) == 1
    assert chunks[0][0].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert chunks[0][0].rate == Decimal("0.0001")
    assert len(chunks[1]) == 1
    assert chunks[1][0].rate == Decimal("0.00015")


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 8],
        [8, None],
        [8, 8],
    ],
)
def test_tardis_load_funding_rates_with_precision(
    price_precision: int | None,
    size_precision: int | None,
):
    """
    Test loading funding rates with different precision settings.
    """
    # Arrange
    filepath = get_test_data_path("derivative_ticker_1.csv")
    instrument_id = InstrumentId.from_str("XBTUSD.BITMEX")
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
        instrument_id=instrument_id,
    )

    # Act
    funding_rates = loader.load_funding_rates(filepath)

    # Assert
    assert len(funding_rates) == 2
    assert funding_rates[0].instrument_id == instrument_id
    assert funding_rates[0].rate == Decimal("0.0001")
    assert funding_rates[1].rate == Decimal("0.00015")


def test_tardis_load_funding_rates_with_limit():
    """
    Test load_funding_rates with limit parameter.
    """
    # Create synthetic CSV data with 5 records
    csv_data = (
        "exchange,symbol,timestamp,local_timestamp,funding_timestamp,"
        "funding_rate,predicted_funding_rate,open_interest,last_price,index_price,mark_price\n"
        "bitmex,XBTUSD,1583020803145000,1583020803307160,1583020800000000,0.0001,0.00012,1000000,9500.5,9500.0,9500.25\n"
        "bitmex,XBTUSD,1583020863145000,1583020863307160,1583020860000000,0.00015,0.00018,1000500,9501.0,9500.5,9500.75\n"
        "bitmex,XBTUSD,1583020923145000,1583020923307160,1583020920000000,0.0002,0.00022,1001000,9501.5,9501.0,9501.25\n"
        "bitmex,XBTUSD,1583020983145000,1583020983307160,1583020980000000,0.00025,0.00028,1001500,9502.0,9501.5,9501.75\n"
        "bitmex,XBTUSD,1583021043145000,1583021043307160,1583021040000000,0.0003,0.00032,1002000,9502.5,9502.0,9502.25"
    )

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test limit of 3 records
        funding_rates = loader.load_funding_rates(temp_file, limit=3)
        assert len(funding_rates) == 3

        # Verify we get the first 3 records in order
        assert funding_rates[0].rate == Decimal("0.0001")
        assert funding_rates[1].rate == Decimal("0.00015")
        assert funding_rates[2].rate == Decimal("0.0002")

        # Test limit larger than available records
        funding_rates_all = loader.load_funding_rates(temp_file, limit=10)
        assert len(funding_rates_all) == 5  # Should return all 5 records

        # Test limit of 1
        funding_rates_one = loader.load_funding_rates(temp_file, limit=1)
        assert len(funding_rates_one) == 1
        assert funding_rates_one[0].rate == Decimal("0.0001")

    finally:
        os.unlink(temp_file)


def test_tardis_stream_funding_rates_with_limit():
    """
    Test stream_funding_rates with limit parameter.
    """
    # Create synthetic CSV data with 8 records
    csv_data = (
        "exchange,symbol,timestamp,local_timestamp,funding_timestamp,"
        "funding_rate,predicted_funding_rate,open_interest,last_price,index_price,mark_price\n"
        "bitmex,XBTUSD,1583020803145000,1583020803307160,1583020800000000,0.0001,0.00012,1000000,9500.5,9500.0,9500.25\n"
        "bitmex,XBTUSD,1583020863145000,1583020863307160,1583020860000000,0.00015,0.00018,1000500,9501.0,9500.5,9500.75\n"
        "bitmex,XBTUSD,1583020923145000,1583020923307160,1583020920000000,0.0002,0.00022,1001000,9501.5,9501.0,9501.25\n"
        "bitmex,XBTUSD,1583020983145000,1583020983307160,1583020980000000,0.00025,0.00028,1001500,9502.0,9501.5,9501.75\n"
        "bitmex,XBTUSD,1583021043145000,1583021043307160,1583021040000000,0.0003,0.00032,1002000,9502.5,9502.0,9502.25\n"
        "bitmex,XBTUSD,1583021103145000,1583021103307160,1583021100000000,0.00035,0.00038,1002500,9503.0,9502.5,9502.75\n"
        "bitmex,XBTUSD,1583021163145000,1583021163307160,1583021160000000,0.0004,0.00042,1003000,9503.5,9503.0,9503.25\n"
        "bitmex,XBTUSD,1583021223145000,1583021223307160,1583021220000000,0.00045,0.00048,1003500,9504.0,9503.5,9503.75"
    )

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test streaming with limit of 5 records, chunk size 3
        limited_chunks = list(loader.stream_funding_rates(temp_file, chunk_size=3, limit=5))
        total_limited = sum(len(chunk) for chunk in limited_chunks)

        # Should only process 5 records due to limit
        assert total_limited == 5

        # Should have 2 chunks: [3, 2] = 5 records total (limited)
        assert len(limited_chunks) == 2
        assert len(limited_chunks[0]) == 3
        assert len(limited_chunks[1]) == 2

        # Test without limit should process all 8 records
        all_chunks = list(loader.stream_funding_rates(temp_file, chunk_size=3))
        total_from_streaming = sum(len(chunk) for chunk in all_chunks)

        # Should be 3 chunks: [3, 3, 2] = 8 records total
        assert len(all_chunks) == 3
        assert total_from_streaming == 8

        # Verify the limited data contains the first 5 records
        all_limited_funding_rates = []
        for chunk in limited_chunks:
            all_limited_funding_rates.extend(chunk)

        assert len(all_limited_funding_rates) == 5
        assert all_limited_funding_rates[0].rate == Decimal("0.0001")
        assert all_limited_funding_rates[1].rate == Decimal("0.00015")
        assert all_limited_funding_rates[4].rate == Decimal("0.0003")  # 5th record

    finally:
        os.unlink(temp_file)


def test_tardis_stream_funding_rates():
    """
    Test streaming functionality for funding rates.
    """
    # Create synthetic CSV data for funding rates
    csv_data = (
        "exchange,symbol,timestamp,local_timestamp,funding_timestamp,"
        "funding_rate,predicted_funding_rate,open_interest,last_price,index_price,mark_price\n"
        "bitmex,XBTUSD,1583020803145000,1583020803307160,1583020800000000,0.0001,0.00012,1000000,9500.5,9500.0,9500.25\n"
        "bitmex,XBTUSD,1583020863145000,1583020863307160,1583020860000000,0.00015,0.00018,1000500,9501.0,9500.5,9500.75\n"
        "bitmex,XBTUSD,1583020923145000,1583020923307160,1583020920000000,0.0002,0.00022,1001000,9501.5,9501.0,9501.25\n"
        "bitmex,XBTUSD,1583020983145000,1583020983307160,1583020980000000,0.00025,0.00028,1001500,9502.0,9501.5,9501.75"
    )

    # Use a temporary file
    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        # Create loader with precision inference
        loader = TardisCSVDataLoader(
            price_precision=None,  # Let it infer
            size_precision=None,  # Let it infer
            instrument_id=InstrumentId.from_str("XBTUSD.BITMEX"),
        )

        # Test streaming with chunk size of 2
        chunks = []
        chunk_count = 0

        for chunk in loader.stream_funding_rates(temp_file, chunk_size=2):
            chunks.append(chunk)
            chunk_count += 1

            # Verify chunk properties
            assert isinstance(chunk, list)
            assert len(chunk) <= 2  # Should not exceed chunk size
            assert len(chunk) > 0  # Should not be empty

            # Verify funding rate properties
            for funding_rate in chunk:
                assert funding_rate.instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
                assert funding_rate.rate is not None
                assert funding_rate.ts_event is not None
                assert funding_rate.ts_init is not None

        # Verify chunking worked correctly
        assert chunk_count == 2  # 4 records / 2 per chunk = 2 chunks
        assert len(chunks[0]) == 2  # First chunk: 2 records
        assert len(chunks[1]) == 2  # Second chunk: 2 records

        # Verify total records
        total_funding_rates = sum(len(chunk) for chunk in chunks)
        assert total_funding_rates == 4

        print(
            f"Funding rate streaming test: {chunk_count} chunks processed, {total_funding_rates} total funding rates",
        )
    finally:
        os.unlink(temp_file)


def test_tardis_stream_funding_rates_memory_efficient():
    """
    Test that funding rate streaming is memory efficient.
    """
    # Create larger synthetic dataset
    csv_rows = [
        "exchange,symbol,timestamp,local_timestamp,funding_timestamp,funding_rate,predicted_funding_rate,open_interest,last_price,index_price,mark_price",
    ]

    # Generate 100 records
    for i in range(100):
        timestamp = 1583020803145000 + i * 60000000  # 60 second intervals (microseconds)
        local_timestamp = timestamp + 162000  # ~162ms delay
        funding_rate = 0.0001 + (i % 50) * 0.00001
        predicted_funding_rate = funding_rate + 0.00002
        funding_timestamp = timestamp - 300000000  # 5 minutes earlier (microseconds)
        open_interest = 1000000 + i * 500
        last_price = 9500.5 + i * 0.5
        index_price = last_price - 0.5
        mark_price = last_price - 0.25

        csv_rows.append(
            f"bitmex,XBTUSD,{timestamp},{local_timestamp},{funding_timestamp},{funding_rate},{predicted_funding_rate},{open_interest},{last_price},{index_price},{mark_price}",
        )

    csv_data = "\n".join(csv_rows)

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader()

        # Test that we can process large file in small chunks
        process = psutil.Process(os.getpid())
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB

        chunk_count = 0
        total_processed = 0

        # Process in very small chunks to test memory efficiency
        for chunk in loader.stream_funding_rates(temp_file, chunk_size=10):
            chunk_count += 1
            total_processed += len(chunk)

            # Memory check - should stay relatively stable
            current_memory = process.memory_info().rss / 1024 / 1024
            memory_increase = current_memory - initial_memory

            # Should not use excessive memory even with 100 records
            assert memory_increase < 100, f"Memory usage too high: {memory_increase:.2f} MB"

        assert total_processed == 100
        assert chunk_count == 10  # 100 / 10 = 10 chunks

        print(
            f"Funding rate memory efficiency test: {total_processed} records in {chunk_count} chunks",
        )
    finally:
        os.unlink(temp_file)


def test_precision_inference_funding_rates():
    """
    Test precision inference with funding rate data.
    """
    # Arrange
    csv_data = (
        "exchange,symbol,timestamp,local_timestamp,funding_timestamp,"
        "funding_rate,predicted_funding_rate,open_interest,last_price,index_price,mark_price\n"
        "bitmex,XBTUSD,1583020803145000,1583020803307160,1583020800000000,0.0001,0.00012,1000000,9500.5,9500.0,9500.25\n"
        "bitmex,XBTUSD,1583020863145000,1583020863307160,1583020860000000,0.000123,0.000145,1000500,9501.0,9500.5,9500.75"
    )

    with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
        f.write(csv_data)
        temp_file = f.name

    try:
        loader = TardisCSVDataLoader(
            price_precision=None,  # Infer
            size_precision=None,  # Infer
        )

        # Act
        funding_rates = loader.load_funding_rates(temp_file)

        # Assert
        assert len(funding_rates) == 2

        # Both funding rates should use the highest precision found (6 decimal places from 0.000123)
        assert funding_rates[0].rate == Decimal("0.0001")
        assert funding_rates[1].rate == Decimal("0.000123")

    finally:
        os.unlink(temp_file)
