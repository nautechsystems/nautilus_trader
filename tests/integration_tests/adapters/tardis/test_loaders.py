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


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")


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
    """
    Test that streaming is memory efficient for all data types.
    """
    test_cases = [
        ("deltas", _generate_deltas_csv),
        ("quotes", _generate_quotes_csv),
        ("trades", _generate_trades_csv),
        ("depth10_snapshot5", _generate_depth10_snapshot5_csv),
        ("depth10_snapshot25", _generate_depth10_snapshot25_csv),
    ]

    for data_type, csv_generator in test_cases:
        _test_memory_efficiency_for_type(data_type, csv_generator)
