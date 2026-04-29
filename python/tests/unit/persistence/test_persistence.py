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
import sys

import pytest

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import HIGH_PRECISION
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
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


def test_backend_session_add_file_and_query_quotes():
    session = DataBackendSession()
    session.add_file(NautilusDataType.QuoteTick, "quotes", _data_path("quotes.parquet"))

    result = session.to_query_result()
    chunk_count = sum(1 for _ in result)

    assert chunk_count > 0


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


def test_catalog_write_and_read_bars(tmp_path):
    path = str(tmp_path / "catalog")
    os.makedirs(path, exist_ok=True)
    catalog = ParquetDataCatalog(path)

    catalog.write_bars([_make_bar(1), _make_bar(2)])

    bar_type_str = str(AUDUSD_1_MIN_BID)
    intervals = catalog.get_intervals("bars", bar_type_str)
    assert intervals == [(1, 2)]


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
    assert intervals == [(1, 2)]


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
    assert intervals == [(1, 2)]


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

    assert len(read) == 1
    assert str(read[0].id) == "AUD/USD.SIM"


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


def _make_stream_signal_class():
    """
    Build a minimal custom data class for streaming tests.
    """
    import json
    from dataclasses import dataclass

    import pyarrow as pa

    @dataclass
    class StreamSignal:
        ts_event: int = 0
        ts_init: int = 0
        label: str = ""
        value: float = 0.0

        @classmethod
        def type_name_static(cls) -> str:
            return "StreamSignal"

        def to_dict(self):
            return {
                "type": "StreamSignal",
                "label": self.label,
                "value": self.value,
                "ts_event": self.ts_event,
                "ts_init": self.ts_init,
            }

        @classmethod
        def from_dict(cls, data):
            data = dict(data)
            data.pop("type", None)
            data.pop("data_type", None)
            return cls(**data)

        def to_json(self):
            return json.dumps(self.to_dict())

        @classmethod
        def from_json(cls, data):
            return cls.from_dict(data)

        def to_arrow(self):
            return pa.RecordBatch.from_pylist(
                [self.to_dict()],
                schema=StreamSignal._schema,
            )

        def encode_record_batch_py(self, items):
            dicts = [x.to_dict() for x in items]
            return pa.RecordBatch.from_pylist(dicts, schema=StreamSignal._schema)

        @classmethod
        def decode_record_batch_py(cls, metadata, batch):
            return [cls.from_dict(d) for d in batch.to_pylist()]

    StreamSignal._schema = pa.schema(
        {
            "label": pa.string(),
            "value": pa.float64(),
            "type": pa.string(),
            "ts_event": pa.int64(),
            "ts_init": pa.int64(),
        },
    )
    return StreamSignal


@pytest.mark.skipif(sys.platform == "win32", reason="SIGALRM not available on Windows")
def test_backend_session_streams_custom_data_without_deadlock(tmp_path):
    """Regression test for GH-3847: streaming custom data deadlocked."""
    import glob
    import signal

    from nautilus_trader.model import CustomData
    from nautilus_trader.model import DataType
    from nautilus_trader.model import register_custom_data_class

    StreamSignal = _make_stream_signal_class()
    register_custom_data_class(StreamSignal)

    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    metadata = {"source": "test"}
    data_type = DataType("StreamSignal", metadata, None)
    items = [StreamSignal(i, i, "sig", float(i)) for i in range(1, 101)]
    wrapped = [CustomData(data_type, item) for item in items]
    catalog.write_custom_data(wrapped)

    session = DataBackendSession(chunk_size=50)
    parquet_files = glob.glob(str(tmp_path / "catalog" / "**" / "*.parquet"), recursive=True)
    assert len(parquet_files) > 0

    for f in parquet_files:
        session.add_custom_file("StreamSignal", "stream_signal", f)

    def timeout_handler(signum, frame):
        raise TimeoutError("DataBackendSession.to_query_result() deadlocked (GH-3847)")

    old_handler = signal.signal(signal.SIGALRM, timeout_handler)
    signal.alarm(10)

    try:
        result = session.to_query_result()
        chunks = list(result)
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, old_handler)

    total = sum(len(chunk) for chunk in chunks)
    assert total == 100
