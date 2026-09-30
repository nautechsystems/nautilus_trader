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
Test persistence behavior.
"""

import datetime as dt
import os
from decimal import Decimal
from pathlib import Path
from zoneinfo import ZoneInfo

import pandas as pd
import pytest

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDepth
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.persistence import BarDataWrangler
from nautilus_trader.persistence import DataCatalogConfig
from nautilus_trader.persistence import OrderBookDeltaDataWrangler
from nautilus_trader.persistence import OrderBookDepthDataWrangler
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import QuoteTickDataWrangler
from nautilus_trader.persistence import RotationConfig
from nautilus_trader.persistence import StreamingFeatherWriter
from nautilus_trader.persistence import StreamingWriter
from nautilus_trader.persistence import TradeTickDataWrangler
from tests.providers import TEST_DATA_DIR
from tests.providers import TestInstrumentProvider
from tests.stubs import TestDataProviderPyo3


AUDUSD_SIM = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)
ARROW_FIXTURES = TEST_DATA_DIR / "nautilus" / "arrow"


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


def test_nautilus_data_type_variants() -> None:
    """
    Test nautilus data type variants.
    """
    assert NautilusDataType.OrderBookDelta is not None
    assert NautilusDataType.OrderBookDepth is not None
    assert NautilusDataType.QuoteTick is not None
    assert NautilusDataType.TradeTick is not None
    assert NautilusDataType.Bar is not None
    assert NautilusDataType.MarkPriceUpdate is not None


def test_catalog_construction(tmp_path: Path) -> None:
    """
    Test catalog construction.
    """
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
def test_catalog_construction_rejects_malformed_uri(uri: object, message: object) -> None:
    """
    Test catalog construction rejects malformed uri.
    """
    with pytest.raises(OSError, match=message):
        ParquetDataCatalog(uri)


def test_catalog_query_custom_data_rejects_non_custom_data_type(tmp_path: Path) -> None:
    """
    Test catalog query custom data rejects a built-in data type.
    """
    catalog = ParquetDataCatalog(str(tmp_path))

    with pytest.raises(TypeError, match="data_type must be a custom NautilusDataType"):
        catalog.query_custom_data(NautilusDataType.QuoteTick)


def test_catalog_write_and_read_bars(tmp_path: Path) -> None:
    """
    Test catalog write and read bars.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals(data_type=NautilusDataType.Bar, identifier=bar_type_str)
    loaded = catalog.query_bars(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == [_make_bar(1), _make_bar(2)]


def test_catalog_write_and_read_quotes(tmp_path: Path) -> None:
    """
    Test catalog write and read quotes.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=AUDUSD_SIM, ts_event=1, ts_init=1),
        TestDataProviderPyo3.quote_tick(instrument_id=AUDUSD_SIM, ts_event=2, ts_init=2),
    ]
    catalog.write_quote_ticks(quotes)

    intervals = catalog.get_intervals(NautilusDataType.QuoteTick, "AUD/USD.SIM")
    loaded = catalog.query_quote_ticks(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == quotes


def test_catalog_write_and_read_trades(tmp_path: Path) -> None:
    """
    Test catalog write and read trades.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    trades = [
        TestDataProviderPyo3.trade_tick(instrument_id=AUDUSD_SIM, ts_event=1, ts_init=1),
        TestDataProviderPyo3.trade_tick(instrument_id=AUDUSD_SIM, ts_event=2, ts_init=2),
    ]
    catalog.write_trade_ticks(trades)

    intervals = catalog.get_intervals(NautilusDataType.TradeTick, "AUD/USD.SIM")
    loaded = catalog.query_trade_ticks(["AUD/USD.SIM"])

    assert intervals == [(1, 2)]
    assert loaded == trades


def test_catalog_write_and_read_order_book_deltas(tmp_path: Path) -> None:
    """
    Test catalog write and read order book deltas.
    """
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


def test_catalog_write_and_read_order_book_depths(tmp_path: Path) -> None:
    """
    Test catalog write and read order book depths.
    """
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
        OrderBookDepth(
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
                assert actual_order.order_id == expected_order.order_id


def test_catalog_append_data(tmp_path: Path) -> None:
    """
    Test catalog append data.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(3)])

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals(NautilusDataType.Bar, bar_type_str)
    assert intervals == [(1, 2), (3, 3)]


def test_catalog_consolidate(tmp_path: Path) -> None:
    """
    Test catalog consolidate.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(3)])
    catalog.consolidate_catalog()

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals(NautilusDataType.Bar, bar_type_str)
    assert intervals == [(1, 3)]


def test_catalog_file_operations_take_identifier_keyword(tmp_path: Path) -> None:
    """
    Test catalog file operations select data with the `identifier` keyword.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    bar_type = str(AUDUSD_1_MIN_BID)
    catalog.write_bars([_make_bar(1), _make_bar(2)])
    catalog.write_bars([_make_bar(5), _make_bar(6)])

    catalog.extend_file_name(NautilusDataType.Bar, identifier=bar_type, start=7, end=7)
    extended = catalog.get_intervals(NautilusDataType.Bar, identifier=bar_type)
    catalog.reset_data_file_names(NautilusDataType.Bar, identifier=bar_type)
    reset = catalog.get_intervals(NautilusDataType.Bar, identifier=bar_type)
    catalog.consolidate_data(NautilusDataType.Bar, identifier=bar_type)
    consolidated = catalog.get_intervals(NautilusDataType.Bar, identifier=bar_type)
    missing = catalog.get_missing_intervals_for_request(
        0,
        10,
        NautilusDataType.Bar,
        identifier=bar_type,
    )
    files = catalog.list_parquet_files(NautilusDataType.Bar, identifier=bar_type)

    assert extended == [(1, 2), (5, 7)]
    assert reset == [(1, 2), (5, 6)]
    assert consolidated == [(1, 6)]
    assert missing == [(0, 0), (7, 10)]
    assert len(files) == 1
    with pytest.raises(TypeError, match="instrument_id"):
        catalog.get_intervals(NautilusDataType.Bar, instrument_id=bar_type)  # type: ignore[call-arg]


def test_catalog_instrument_roundtrip(tmp_path: Path) -> None:
    """
    Test catalog instrument roundtrip.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    base = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    payload = {**CurrencyPair.to_dict(base), "ts_event": 1000, "ts_init": 1000}
    inst = CurrencyPair.from_dict(payload)

    catalog.write_instruments([inst])
    read = catalog.instruments(instrument_ids=["AUD/USD.SIM"])

    assert [instrument.to_dict() for instrument in read] == [inst.to_dict()]


def test_catalog_list_parquet_files_with_typed_selectors(tmp_path: Path) -> None:
    """
    Test listing parquet files with typed selectors.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=AUDUSD_SIM, ts_event=1, ts_init=1),
    ]
    catalog.write_quote_ticks(quotes)
    currency_pair = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    equity = TestInstrumentProvider.aapl_equity()
    catalog.write_instruments([currency_pair, equity])

    quote_files = catalog.list_parquet_files(NautilusDataType.QuoteTick, "AUDUSD.SIM")

    assert len(quote_files) == 1
    assert "data/quotes/AUDUSD.SIM/" in quote_files[0]

    pair_files = catalog.list_parquet_files(NautilusDataType.Instrument, "AUDUSD.SIM")
    equity_files = catalog.list_parquet_files(NautilusDataType.Instrument, "AAPL.XNAS")

    assert len(pair_files) == 1
    assert "data/currency_pair/AUDUSD.SIM/" in pair_files[0]
    assert len(equity_files) == 1
    assert "data/equity/AAPL.XNAS/" in equity_files[0]


def test_catalog_query_filters_and_timestamp_metadata(tmp_path: Path) -> None:
    """
    Test catalog query filters and timestamp metadata.
    """
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
        where_clause="ts_init >= arrow_cast(5, 'Timestamp(Nanosecond, Some(\"UTC\"))')",
    )

    assert loaded == [_make_bar(5), _make_bar(6)]
    assert catalog.query_first_timestamp(data_type=NautilusDataType.Bar, identifier=bar_type) == 1
    assert catalog.query_last_timestamp(data_type=NautilusDataType.Bar, identifier=bar_type) == 6
    assert catalog.get_missing_intervals_for_request(0, 10, NautilusDataType.Bar, bar_type) == [
        (0, 0),
        (3, 4),
        (7, 10),
    ]
    assert catalog.list_data_types() == [NautilusDataType.Bar]


def test_catalog_delete_data_range_uses_nanosecond_boundaries(tmp_path: Path) -> None:
    """
    Test catalog delete data range uses nanosecond boundaries.
    """
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)
    timestamps = [1_000_000_000, 1_000_000_001, 1_000_000_002, 1_000_000_003]
    catalog.write_bars([_make_bar(ts) for ts in timestamps])

    catalog.delete_data_range(
        NautilusDataType.Bar,
        str(AUDUSD_1_MIN_BID),
        1_000_000_001,
        1_000_000_002,
    )

    loaded = catalog.query_bars(["AUD/USD.SIM"])
    assert [bar.ts_init for bar in loaded] == [1_000_000_000, 1_000_000_003]


def test_catalog_query_handles_multiple_instrument_identifier_patterns(tmp_path: Path) -> None:
    """
    Test catalog query handles multiple instrument identifier patterns.
    """
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


def test_quote_tick_wrangler_construction() -> None:
    """
    Test quote tick wrangler construction.
    """
    wrangler = QuoteTickDataWrangler(
        instrument_id="AUD/USD.SIM",
        price_precision=5,
        size_precision=0,
    )

    assert wrangler.instrument_id == "AUD/USD.SIM"
    assert wrangler.price_precision == 5
    assert wrangler.size_precision == 0


def test_trade_tick_wrangler_construction() -> None:
    """
    Test trade tick wrangler construction.
    """
    wrangler = TradeTickDataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_bar_wrangler_construction() -> None:
    """
    Test bar wrangler construction.
    """
    wrangler = BarDataWrangler(
        bar_type="AUD/USD.SIM-1-MINUTE-BID-EXTERNAL",
        price_precision=5,
        size_precision=0,
    )

    assert wrangler.bar_type == "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"
    assert wrangler.price_precision == 5
    assert wrangler.size_precision == 0


def test_order_book_delta_wrangler_construction() -> None:
    """
    Test order book delta wrangler construction.
    """
    wrangler = OrderBookDeltaDataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_order_book_depth_wrangler_construction() -> None:
    """
    Test order book depth wrangler construction.
    """
    wrangler = OrderBookDepthDataWrangler(
        instrument_id="ETHUSDT.BINANCE",
        price_precision=2,
        size_precision=5,
    )

    assert wrangler.instrument_id == "ETHUSDT.BINANCE"
    assert wrangler.price_precision == 2
    assert wrangler.size_precision == 5


def test_streaming_feather_writer_construction(tmp_path: Path) -> None:
    """
    Test streaming feather writer construction.
    """
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
    )

    assert writer is not None
    assert isinstance(writer.is_closed, bool)


def test_streaming_feather_writer_write_and_flush(tmp_path: Path) -> None:
    """
    Test streaming feather writer write and flush.
    """
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


def test_streaming_feather_writer_write_trade(tmp_path: Path) -> None:
    """
    Test streaming feather writer write trade.
    """
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
    ("data_name", "data_type", "data_factory"),
    [
        (
            "mark_prices",
            NautilusDataType.MarkPriceUpdate,
            lambda instrument_id: MarkPriceUpdate(
                instrument_id,
                Price.from_str("100.00"),
                1_000,
                1_000,
            ),
        ),
        (
            "index_prices",
            NautilusDataType.IndexPriceUpdate,
            lambda instrument_id: IndexPriceUpdate(
                instrument_id,
                Price.from_str("100.00"),
                1_000,
                1_000,
            ),
        ),
        (
            "funding_rates",
            NautilusDataType.FundingRateUpdate,
            lambda instrument_id: FundingRateUpdate(
                instrument_id,
                Decimal("0.0001"),
                1_000,
                1_000,
                interval=480,
                next_funding_ns=2_000,
            ),
        ),
    ],
)
def test_streaming_feather_writer_uses_one_file_per_type(
    tmp_path: Path,
    data_name: object,
    data_type: object,
    data_factory: object,
) -> None:
    """
    Test streaming feather writer stages every instrument of a type in one file.
    """
    path = tmp_path / f"streaming_{data_name}"
    path.mkdir()
    writer = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[data_type],
    )

    writer.write(data_factory(InstrumentId.from_str("ETHUSDT.BINANCE")))
    writer.write(data_factory(InstrumentId.from_str("BTCUSDT.BINANCE")))
    writer.close()

    assert [file.relative_to(path).as_posix() for file in path.rglob("*.feather")] == [
        f"{data_name}/{data_name}_0.feather",
    ]


def test_streaming_feather_writer_replace_removes_local_files(tmp_path: Path) -> None:
    """
    Test streaming feather writer replace removes local files.
    """
    path = tmp_path / "streaming_replace"
    path.mkdir()
    instrument_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    writer = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[NautilusDataType.QuoteTick],
    )
    writer.write(TestDataProviderPyo3.quote_tick(instrument_id=instrument_id))
    writer.close()
    assert len(list(path.glob("quotes/*.feather"))) == 1

    replacement = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[NautilusDataType.QuoteTick],
        replace=True,
    )
    replacement.close()

    assert list(path.glob("quotes/*.feather")) == []


def test_streaming_feather_writer_recovers_partial_files_on_start(tmp_path: Path) -> None:
    """
    Test streaming feather writer seals partial files a crashed writer left.
    """
    path = tmp_path / "streaming_recover"
    path.mkdir()
    instrument_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    writer = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[NautilusDataType.QuoteTick],
    )
    writer.write(TestDataProviderPyo3.quote_tick(instrument_id=instrument_id))
    writer.close()

    # A writer that exited before sealing leaves its flushed stream as a partial file
    [sealed] = path.glob("quotes/*.feather")
    sealed.rename(sealed.with_name(f"{sealed.name}.partial"))

    restarted = StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[NautilusDataType.QuoteTick],
    )
    files = [file.relative_to(path).as_posix() for file in path.rglob("*.feather*")]
    restarted.close()

    assert files == [sealed.relative_to(path).as_posix()]


def test_streaming_feather_writer_rejects_string_include_type(tmp_path: Path) -> None:
    """
    Test streaming feather writer rejects a catalog name as an include type.
    """
    with pytest.raises(
        TypeError,
        match="filter key must be NautilusRecordType or NautilusDataType",
    ):
        StreamingFeatherWriter(
            path=str(tmp_path),
            cache=Cache(),
            clock=Clock.new_test(),
            include_types=["quotes"],
        )


def test_streaming_feather_writer_rejects_remote_path() -> None:
    """
    Test streaming feather writer rejects a remote path.
    """
    with pytest.raises(OSError, match="writer path must be local, was s3://test-bucket/stream"):
        StreamingFeatherWriter(
            path="s3://test-bucket/stream",
            cache=Cache(),
            clock=Clock.new_test(),
        )


def test_streaming_writer_promotes_into_catalog(tmp_path: Path) -> None:
    """
    Test streaming writer with a catalog promotes its Feather files into that catalog.
    """
    catalog_path = tmp_path / "catalog"
    writer = StreamingWriter(
        str(tmp_path / "stream" / "backtest" / "run-1"),
        Clock.new_test(),
        catalog=DataCatalogConfig(path=str(catalog_path)),
    )
    quote = TestDataProviderPyo3.quote_tick()

    writer.write(quote)
    writer.close()

    assert writer.backend == "Parquet"
    assert ParquetDataCatalog(str(catalog_path)).query_quote_ticks() == [quote]


def test_streaming_writer_rejects_unknown_catalog_param(tmp_path: Path) -> None:
    """
    Test streaming writer rejects a catalog param its catalog backend does not accept.
    """
    with pytest.raises(OSError, match="Unknown Parquet catalog param") as exc_info:
        StreamingWriter(
            str(tmp_path / "stream" / "backtest" / "run-1"),
            Clock.new_test(),
            catalog=DataCatalogConfig(
                path=str(tmp_path / "catalog"),
                params={"no_such_param": 1024},
            ),
        )

    assert str(exc_info.value) == (
        "Failed to create writer: Unknown Parquet catalog param 'no_such_param', "
        "expected one of storage_options, batch_size, compression, max_row_group_size"
    )


def test_streaming_writer_without_catalog_keeps_feather_files(tmp_path: Path) -> None:
    """
    Test streaming writer without a catalog keeps only the Feather files.
    """
    path = tmp_path / "stream"
    writer = StreamingWriter(str(path), Clock.new_test())

    writer.write(TestDataProviderPyo3.quote_tick())
    writer.close()

    assert writer.backend == "Feather"
    assert [file.relative_to(path).as_posix() for file in path.rglob("*.feather")] == [
        "quotes/quotes_0.feather",
    ]


def test_streaming_feather_writer_close(tmp_path: Path) -> None:
    """
    Test streaming feather writer close.
    """
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


def test_streaming_feather_writer_rotation_modes(tmp_path: Path) -> None:
    """
    Test streaming feather writer rotation modes.
    """
    cache = Cache()
    clock = Clock.new_test()

    for index, rotation_config in enumerate(
        [
            RotationConfig.size(1024 * 1024),
            RotationConfig.interval(3600_000_000_000),
            RotationConfig.no_rotation(),
            None,
        ],
    ):
        path = str(tmp_path / f"streaming_{index}")
        os.makedirs(path, exist_ok=True)
        writer = StreamingFeatherWriter(
            path=path,
            cache=cache,
            clock=clock,
            rotation_config=rotation_config,
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
    tmp_path: Path,
    now: object,
    expected: object,
) -> None:
    """
    Test streaming feather writer scheduled rotation matches python across dst.
    """
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)
    clock = Clock.new_test()
    clock.set_time(pd.Timestamp(now).value)
    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=clock,
        rotation_config=RotationConfig.scheduled_dates(
            86_400_000_000_000,
            1_800_000_000_000,
            timezone="America/New_York",
        ),
    )
    quote = TestDataProviderPyo3.quote_tick()

    writer.write(quote)

    next_rotation_rust = writer.get_next_rotation_time(NautilusDataType.QuoteTick)
    next_rotation_python = _next_rotation_python(now)
    expected_ns = pd.Timestamp(expected).value

    assert next_rotation_rust == next_rotation_python.value
    assert next_rotation_rust == expected_ns


def _next_rotation_python(now: object) -> object:
    now = pd.Timestamp(now)
    rotation_timezone = ZoneInfo("America/New_York")
    rotation_time = pd.Timestamp.combine(now.date(), dt.time(0, 30))
    next_rotation = pd.Timestamp(rotation_time, tz=rotation_timezone).tz_convert("UTC")

    while next_rotation <= now:
        next_rotation += pd.Timedelta(days=1)

    return next_rotation


def test_streaming_feather_writer_include_types(tmp_path: Path) -> None:
    """
    Test streaming feather writer include types.
    """
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
        include_types=[NautilusDataType.QuoteTick, NautilusDataType.TradeTick],
    )

    assert writer is not None


def test_streaming_feather_writer_flush_interval(tmp_path: Path) -> None:
    """
    Test streaming feather writer flush interval.
    """
    path = str(tmp_path / "streaming")
    os.makedirs(path, exist_ok=True)

    writer = StreamingFeatherWriter(
        path=path,
        cache=Cache(),
        clock=Clock.new_test(),
        flush_interval_ms=500,
    )

    assert writer is not None
