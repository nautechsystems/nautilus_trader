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

import os

import pytest

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDepth10
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.persistence import BarDataWrangler
from nautilus_trader.persistence import DataBackendSession
from nautilus_trader.persistence import NautilusDataType
from nautilus_trader.persistence import OrderBookDeltaDataWrangler
from nautilus_trader.persistence import OrderBookDepth10DataWrangler
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import QuoteTickDataWrangler
from nautilus_trader.persistence import StreamingFeatherWriter
from nautilus_trader.persistence import TradeTickDataWrangler
from tests.providers import TEST_DATA_DIR
from tests.providers import TestInstrumentProvider
from tests.stubs import TestDataProviderPyo3


AUDUSD_SIM = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)


def _data_path(name: str) -> str:
    subdir = "128-bit" if HIGH_PRECISION else "64-bit"
    return str(TEST_DATA_DIR / "nautilus" / subdir / name)


def _make_bar(ts: int) -> Bar:
    return Bar(
        AUDUSD_1_MIN_BID,
        Price.from_str("1.00001"),
        Price.from_str("1.1"),
        Price.from_str("1.00000"),
        Price.from_str("1.00000"),
        Quantity.from_int(100_000),
        ts,
        ts,
    )


def test_backend_session_construction():
    session = DataBackendSession()

    assert session is not None


def test_backend_session_construction_with_chunk_size():
    session = DataBackendSession(chunk_size=5_000)

    assert session is not None


def test_backend_session_rejects_zero_chunk_size():
    with pytest.raises(ValueError, match="chunk_size must be positive"):
        DataBackendSession(chunk_size=0)


def test_backend_session_add_file_and_query_quotes():
    session = DataBackendSession()
    session.add_file(NautilusDataType.QuoteTick, "quotes", _data_path("quotes.parquet"))

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


def test_backend_session_to_list_queries_quotes():
    session = DataBackendSession()
    session.add_file(NautilusDataType.QuoteTick, "quotes", _data_path("quotes.parquet"))

    quotes = session.to_query_result().to_list()

    assert len(quotes) == 9_500
    assert all(isinstance(quote, QuoteTick) for quote in quotes)
    assert quotes[0].ts_init == 1_577_898_000_000_000_065
    assert quotes[-1].ts_init == 1_577_919_652_000_000_125


def test_backend_session_to_list_returns_unread_records():
    session = DataBackendSession(chunk_size=1_000)
    session.add_file(NautilusDataType.QuoteTick, "quotes", _data_path("quotes.parquet"))
    result = session.to_query_result()

    next(result)
    quotes = result.to_list()

    assert len(quotes) == 8_500
    assert all(isinstance(quote, QuoteTick) for quote in quotes)
    assert quotes[0].ts_init == 1_577_900_944_000_000_879


def test_backend_session_to_list_returns_empty_for_empty_query():
    session = DataBackendSession()
    session.add_file(
        NautilusDataType.QuoteTick,
        "quotes",
        _data_path("quotes.parquet"),
        "SELECT * FROM quotes WHERE 1=0",
    )

    assert session.to_query_result().to_list() == []


def test_backend_session_add_file_and_query_trades():
    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trades", _data_path("trades.parquet"))

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


def test_backend_session_add_file_and_query_bars():
    session = DataBackendSession()
    session.add_file(NautilusDataType.Bar, "bars", _data_path("bars.parquet"))

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


def test_backend_session_add_file_and_query_deltas():
    session = DataBackendSession()
    session.add_file(
        NautilusDataType.OrderBookDelta,
        "deltas",
        _data_path("deltas.parquet"),
    )

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


def test_backend_session_multiple_files():
    session = DataBackendSession()
    session.add_file(NautilusDataType.TradeTick, "trades", _data_path("trades.parquet"))
    session.add_file(NautilusDataType.QuoteTick, "quotes", _data_path("quotes.parquet"))

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


def test_backend_session_nautilus_data_type_variants():
    assert NautilusDataType.OrderBookDelta is not None
    assert NautilusDataType.OrderBookDepth10 is not None
    assert NautilusDataType.QuoteTick is not None
    assert NautilusDataType.TradeTick is not None
    assert NautilusDataType.Bar is not None
    assert NautilusDataType.MarkPriceUpdate is not None


def test_catalog_construction(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)

    catalog = ParquetDataCatalog(path)

    assert catalog is not None


@pytest.mark.parametrize(
    ("uri", "message"),
    [
        ("s3://", "Invalid S3 URI: missing bucket"),
        ("gs://", "Invalid GCS URI: missing bucket"),
        ("az://", "Invalid Azure URI: missing container"),
        ("https://", "empty host"),
    ],
)
def test_catalog_construction_rejects_malformed_uri(uri, message):
    with pytest.raises(OSError, match=message):
        ParquetDataCatalog(uri)


def test_catalog_write_and_read_bars(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals("bars", bar_type_str)
    loaded = catalog.query_bars(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == [_make_bar(1), _make_bar(2)]


def test_catalog_write_and_read_quotes(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=AUDUSD_SIM, ts_event=1, ts_init=1),
        TestDataProviderPyo3.quote_tick(instrument_id=AUDUSD_SIM, ts_event=2, ts_init=2),
    ]
    catalog.write_quote_ticks(quotes)

    intervals = catalog.get_intervals("quotes", "AUD/USD.SIM")
    loaded = catalog.query_quote_ticks(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == quotes


def test_catalog_write_and_read_trades(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    trades = [
        TestDataProviderPyo3.trade_tick(instrument_id=AUDUSD_SIM, ts_event=1, ts_init=1),
        TestDataProviderPyo3.trade_tick(instrument_id=AUDUSD_SIM, ts_event=2, ts_init=2),
    ]
    catalog.write_trade_ticks(trades)

    intervals = catalog.get_intervals("trades", "AUD/USD.SIM")
    loaded = catalog.query_trade_ticks(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == trades


def test_catalog_write_and_read_order_book_deltas(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    deltas = [
        OrderBookDelta(
            instrument_id=AUDUSD_SIM,
            action=BookAction.ADD,
            order=BookOrder(
                OrderSide.BUY,
                Price.from_str("1.10001"),
                Quantity.from_str("100.123"),
                42,
            ),
            flags=7,
            sequence=101,
            ts_event=1,
            ts_init=2,
        ),
        OrderBookDelta(
            instrument_id=AUDUSD_SIM,
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                Price.from_str("1.10002"),
                Quantity.from_str("200.456"),
                43,
            ),
            flags=8,
            sequence=102,
            ts_event=3,
            ts_init=4,
        ),
    ]
    catalog.write_order_book_deltas(deltas)

    loaded = catalog.query_order_book_deltas(["AUD/USD.SIM"])

    assert len(loaded) == len(deltas)

    for expected, actual in zip(deltas, loaded, strict=True):
        assert actual.instrument_id == expected.instrument_id
        assert actual.action == expected.action
        assert actual.flags == expected.flags
        assert actual.sequence == expected.sequence
        assert actual.ts_event == expected.ts_event
        assert actual.ts_init == expected.ts_init
        assert actual.order.side == expected.order.side
        assert actual.order.price == expected.order.price
        assert actual.order.size == expected.order.size
        assert actual.order.order_id == expected.order.order_id


def test_catalog_write_and_read_order_book_depths(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    bids = [
        BookOrder(
            OrderSide.BUY,
            Price.from_str(f"{1.10000 - level * 0.00001:.5f}"),
            Quantity.from_str(str(level + 1)),
            level + 1,
        )
        for level in range(10)
    ]
    asks = [
        BookOrder(
            OrderSide.SELL,
            Price.from_str(f"{1.10001 + level * 0.00001:.5f}"),
            Quantity.from_str(str(level + 11)),
            level + 11,
        )
        for level in range(10)
    ]
    depths = [
        OrderBookDepth10(
            instrument_id=AUDUSD_SIM,
            bids=bids,
            asks=asks,
            bid_counts=list(range(1, 11)),
            ask_counts=list(range(11, 21)),
            flags=9,
            sequence=201,
            ts_event=5,
            ts_init=6,
        ),
    ]
    catalog.write_order_book_depths(depths)

    loaded = catalog.query_order_book_depths(["AUD/USD.SIM"])

    assert len(loaded) == len(depths)

    for expected, actual in zip(depths, loaded, strict=True):
        assert actual.instrument_id == expected.instrument_id
        assert actual.bid_counts == expected.bid_counts
        assert actual.ask_counts == expected.ask_counts
        assert actual.flags == expected.flags
        assert actual.sequence == expected.sequence
        assert actual.ts_event == expected.ts_event
        assert actual.ts_init == expected.ts_init

        for expected_orders, actual_orders in (
            (expected.bids, actual.bids),
            (expected.asks, actual.asks),
        ):
            for expected_order, actual_order in zip(expected_orders, actual_orders, strict=True):
                assert actual_order.side == expected_order.side
                assert actual_order.price == expected_order.price
                assert actual_order.size == expected_order.size
                assert expected_order.order_id != 0
                assert actual_order.order_id == 0


def test_catalog_append_data(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(3)])

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals("bars", bar_type_str)
    assert intervals == [(1, 2), (3, 3)]


def test_catalog_consolidate(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(3)])
    catalog.consolidate_catalog()

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals("bars", bar_type_str)
    assert intervals == [(1, 3)]


def test_catalog_instrument_roundtrip(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    base = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    payload = {**CurrencyPair.to_dict(base), "ts_event": 1000, "ts_init": 1000}
    inst = CurrencyPair.from_dict(payload)

    catalog.write_instruments([inst])
    read = catalog.instruments(instrument_ids=["AUD/USD.SIM"])

    assert [instrument.to_dict() for instrument in read] == [inst.to_dict()]


def test_quote_tick_wrangler_construction():
    wrangler = QuoteTickDataWrangler(
        instrument_id="AUD/USD.SIM",
        price_precision=5,
        size_precision=0,
    )

    assert wrangler.instrument_id == "AUD/USD.SIM"
    assert wrangler.price_precision == 5
    assert wrangler.size_precision == 0


def test_trade_tick_wrangler_construction():
    wrangler = TradeTickDataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_bar_wrangler_construction():
    wrangler = BarDataWrangler(
        bar_type="AUD/USD.SIM-1-MINUTE-BID-EXTERNAL",
        price_precision=5,
        size_precision=0,
    )

    assert wrangler.bar_type == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"
    assert wrangler.price_precision == 5
    assert wrangler.size_precision == 0


def test_order_book_delta_wrangler_construction():
    wrangler = OrderBookDeltaDataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_order_book_depth10_wrangler_construction():
    wrangler = OrderBookDepth10DataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_streaming_feather_writer_construction(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
    )

    assert writer is not None
    assert isinstance(writer.is_closed, bool)


def test_streaming_feather_writer_write_and_flush(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
    )
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)
    writer.flush()


def test_streaming_feather_writer_write_trade(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
    )
    trade = TestDataProviderPyo3.trade_tick()
    writer.write(trade)
    writer.flush()


def test_streaming_feather_writer_close(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
    )
    quote = TestDataProviderPyo3.quote_tick()
    writer.write(quote)
    writer.close()

    assert writer.is_closed


def test_streaming_feather_writer_rotation_modes(tmp_path):
    cache = Cache()
    clock = Clock.new_test()

    for mode, kwargs in [
        (0, {"max_file_size": 1024 * 1024}),
        (1, {"rotation_interval_ns": 3600_000_000_000}),
        (3, {}),
    ]:
        path = str(tmp_path / f"streaming_{mode}")
        os.makedirs(path, exist_ok=True)
        writer = StreamingFeatherWriter(
            path=path,
            cache=cache,
            clock=clock,
            rotation_mode=mode,
            **kwargs,
        )
        assert writer is not None


def test_streaming_feather_writer_include_types(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=["quotes", "trades"],
    )

    assert writer is not None


def test_streaming_feather_writer_flush_interval(tmp_path):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
        flush_interval_ms=500,
    )

    assert writer is not None
