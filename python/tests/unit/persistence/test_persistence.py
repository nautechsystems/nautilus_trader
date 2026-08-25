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

import datetime as dt
import os
from decimal import Decimal
from zoneinfo import ZoneInfo

import pandas as pd
import pyarrow as pa
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
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
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
        Price.from_str("1.10000"),
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

    chunks = list(session.to_query_result())
    quotes = chunks[0]

    assert len(chunks) == 1
    assert isinstance(quotes, list)
    assert len(quotes) == 9_500
    assert all(isinstance(quote, QuoteTick) for quote in quotes)
    assert quotes[0].ts_init == 1_577_898_000_000_000_065
    assert quotes[-1].ts_init == 1_577_919_652_000_000_125


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


def test_catalog_query_filters_and_timestamp_metadata(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    bar_type = str(AUDUSD_1_MIN_BID)
    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(5), _make_bar(6)])

    loaded = catalog.query_bars(
        ["AUD/USD.SIM"],
        start=1,
        end=6,
        where_clause="ts_init >= 5",
    )

    assert loaded == [_make_bar(5), _make_bar(6)]
    assert catalog.query_first_timestamp("bars", bar_type) == 1
    assert catalog.query_last_timestamp("bars", bar_type) == 6
    assert catalog.get_missing_intervals_for_request(0, 10, "bars", bar_type) == [
        (0, 0),
        (3, 4),
        (7, 10),
    ]
    assert "bars" in catalog.list_data_types()


def test_catalog_delete_data_range_uses_nanosecond_boundaries(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    timestamps = [1_000_000_000, 1_000_000_001, 1_000_000_002, 1_000_000_003]
    catalog.write_bars([_make_bar(ts) for ts in timestamps])

    catalog.delete_data_range(
        "bars",
        str(AUDUSD_1_MIN_BID),
        1_000_000_001,
        1_000_000_002,
    )

    loaded = catalog.query_bars(["AUD/USD.SIM"])
    assert [bar.ts_init for bar in loaded] == [1_000_000_000, 1_000_000_003]


def test_catalog_query_handles_multiple_instrument_identifier_patterns(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    instrument_ids = [
        InstrumentId.from_str("EUR/USD.SIM"),
        InstrumentId.from_str("BTC-USD.COINBASE"),
        InstrumentId.from_str("ETH/USDT.BINANCE"),
    ]
    quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=instrument_id, ts_event=i, ts_init=i)
        for i, instrument_id in enumerate(instrument_ids, start=1)
    ]

    for quote in quotes:
        catalog.write_quote_ticks([quote])

    loaded = catalog.query_quote_ticks([str(instrument_id) for instrument_id in instrument_ids])

    assert loaded == quotes


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


@pytest.mark.skipif(os.name == "nt", reason="Feather stream path checks are not stable on Windows")
@pytest.mark.parametrize(
    ("data_name", "data_factory", "expected_metadata"),
    [
        (
            "mark_prices",
            lambda instrument_id: MarkPriceUpdate(
                instrument_id,
                Price.from_str("100.00"),
                1_000,
                1_000,
            ),
            {b"price_precision": b"2"},
        ),
        (
            "index_prices",
            lambda instrument_id: IndexPriceUpdate(
                instrument_id,
                Price.from_str("100.00"),
                1_000,
                1_000,
            ),
            {b"price_precision": b"2"},
        ),
        (
            "funding_rate_update",
            lambda instrument_id: FundingRateUpdate(
                instrument_id,
                Decimal("0.0001"),
                1_000,
                1_000,
                interval=480,
                next_funding_ns=2_000,
            ),
            {b"type": b"FundingRateUpdate"},
        ),
    ],
)
def test_streaming_feather_writer_uses_per_instrument_paths(
    tmp_path,
    data_name,
    data_factory,
    expected_metadata,
):
    path = tmp_path / f"streaming_{data_name}"
    path.mkdir()
    instrument_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    writer = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[data_name],
    )

    writer.write(data_factory(instrument_id))
    writer.close()

    files = list(path.glob(f"{data_name}/{instrument_id}/*.feather"))
    assert len(files) == 1
    with files[0].open("rb") as stream:
        table = pa.ipc.open_stream(stream).read_all()
    assert table.schema.metadata is not None
    assert table.schema.metadata[b"instrument_id"] == str(instrument_id).encode()
    for key, value in expected_metadata.items():
        assert table.schema.metadata[key] == value


@pytest.mark.parametrize(
    ("data_name", "event"),
    [
        (
            "instrument_status",
            InstrumentStatus(AUDUSD_SIM, MarketStatusAction.TRADING, 1_000, 1_001),
        ),
        (
            "instrument_closes",
            InstrumentClose(
                AUDUSD_SIM,
                Price.from_str("1.00001"),
                InstrumentCloseType.END_OF_SESSION,
                2_000,
                2_001,
            ),
        ),
    ],
)
def test_streaming_feather_writer_status_and_close_catalog_round_trip(
    tmp_path,
    data_name,
    event,
):
    instance_id = "test_instance"
    stream_path = tmp_path / "live" / instance_id
    stream_path.mkdir(parents=True)
    writer = StreamingFeatherWriter(
        path=str(stream_path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[data_name],
        flush_interval_ms=0,
    )

    writer.write(event)
    writer.close()

    files = list((stream_path / data_name).glob("*/*.feather"))
    assert len(files) == 1
    with files[0].open("rb") as stream:
        table = pa.ipc.open_stream(stream).read_all()
    assert table.schema.metadata is not None
    assert table.schema.metadata[b"instrument_id"] == str(AUDUSD_SIM).encode()

    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.convert_stream_to_data(instance_id, data_name, subdirectory="live")

    assert catalog.query(data_name, identifiers=[str(AUDUSD_SIM)]) == [event]


def test_streaming_feather_writer_replace_removes_local_files(tmp_path):
    path = tmp_path / "streaming_replace"
    path.mkdir()
    instrument_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    writer = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=["quotes"],
    )
    writer.write(TestDataProviderPyo3.quote_tick(instrument_id=instrument_id))
    writer.close()
    assert len(list(path.glob(f"quotes/{instrument_id}/*.feather"))) == 1

    replacement = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=["quotes"],
        replace=True,
    )
    replacement.close()

    assert list(path.glob(f"quotes/{instrument_id}/*.feather")) == []


def test_streaming_feather_writer_replace_rejects_remote_root():
    with pytest.raises(
        OSError,
        match="replace=True for remote streaming paths requires a non-empty prefix",
    ):
        StreamingFeatherWriter(
            path="test-bucket",
            cache=Cache(),
            clock=Clock.new_test(),
            fs_protocol="s3",
            fs_storage_options={
                "access_key_id": "not-a-key",
                "secret_access_key": "not-a-secret",
            },
            replace=True,
        )


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


@pytest.mark.parametrize(
    ("now", "expected"),
    [
        (
            dt.datetime(2026, 3, 8, 7, 30, tzinfo=dt.UTC),
            dt.datetime(2026, 3, 9, 5, 30, tzinfo=dt.UTC),
        ),
        (
            dt.datetime(2026, 11, 1, 6, 30, tzinfo=dt.UTC),
            dt.datetime(2026, 11, 2, 4, 30, tzinfo=dt.UTC),
        ),
    ],
    ids=["cross_gap", "cross_fold"],
)
def test_streaming_feather_writer_scheduled_rotation_matches_python_across_dst(
    tmp_path,
    now,
    expected,
):
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)
    clock = Clock.new_test()
    clock.set_time(pd.Timestamp(now).value)
    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=clock,
        rotation_mode=2,
        rotation_interval_ns=86_400_000_000_000,
        rotation_time_ns=1_800_000_000_000,
        rotation_timezone="America/New_York",
    )
    quote = TestDataProviderPyo3.quote_tick()

    writer.write(quote)

    next_rotation_rust = writer.get_next_rotation_time("quotes", str(quote.instrument_id))
    next_rotation_python = _next_rotation_python(now)
    expected_ns = pd.Timestamp(expected).value

    assert next_rotation_rust == next_rotation_python.value
    assert next_rotation_rust == expected_ns


def _next_rotation_python(now):
    now = pd.Timestamp(now)
    rotation_timezone = ZoneInfo("America/New_York")
    rotation_time = pd.Timestamp.combine(now.date(), dt.time(0, 30))
    next_rotation = pd.Timestamp(rotation_time, tz=rotation_timezone).tz_convert("UTC")

    while next_rotation <= now:
        next_rotation += pd.Timedelta(days=1)

    return next_rotation


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
