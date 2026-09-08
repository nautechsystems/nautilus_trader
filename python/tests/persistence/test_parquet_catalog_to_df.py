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
Parquet catalog regression tests.
"""

import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pyarrow as pa
import pytest
from tests.providers import TestInstrumentProvider as TestInstrumentProviderPyo3
from tests.stubs import TestDataProviderPyo3

from nautilus_trader import model
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import NautilusInstrumentType
from nautilus_trader.model import NautilusRecordType
from nautilus_trader.model import Price
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.persistence import RustTestPriceMapCustomData
from nautilus_trader.persistence.catalog_to_df import CatalogOutput
from nautilus_trader.persistence.catalog_to_df import query_catalog


pytestmark = pytest.mark.skipif(
    sys.platform == "win32",
    reason="Lakehouse local paths use Unix paths",
)


def _write_and_query(
    tmp_path: Path,
    catalog_backend: str,
    data_type: NautilusDataType,
    writer_name: str,
    data: object,
    output: CatalogOutput | None = None,
):
    path = tmp_path / catalog_backend / str(data_type)
    path.mkdir(parents=True, exist_ok=True)
    if catalog_backend == "parquet":
        catalog = ParquetDataCatalog(str(path))
        getattr(catalog, writer_name)([data])
        if output is None:
            return query_catalog(catalog, data_type)
        return query_catalog(catalog, data_type, output=output)

    catalog = ParquetDataCatalog(str(path))
    getattr(catalog, writer_name)([data])
    if output is None:
        return query_catalog(catalog, data_type)
    return query_catalog(catalog, data_type, output=output)


def _columns(df) -> list[str]:
    return list(df.columns)


def _height(df) -> int:
    if hasattr(df, "height"):
        return df.height
    return len(df)


def _record_batch_arrow_bytes() -> bytes:
    batch = pa.record_batch(
        [
            pa.array([10, 20, 30], type=pa.uint64()),
            pa.array([1, 2, 3], type=pa.int32()),
        ],
        names=["ts_init", "value"],
    )
    sink = pa.BufferOutputStream()
    with pa.ipc.new_stream(sink, batch.schema) as writer:
        writer.write_batch(batch)
    return sink.getvalue().to_pybytes()


def test_customdataclass_round_trips_through_registered_arrow_schema(tmp_path: Path) -> None:
    """
    Verify customdataclass round trips through registered arrow schema.
    """

    @customdataclass()
    class PythonCatalogSignal:
        value: float = 0.0
        source: str = ""

    register_custom_data_class(PythonCatalogSignal)
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("PythonCatalogSignal", identifier="TEST")
    signal = PythonCatalogSignal(11, 12, 42.5, "python")

    catalog.write_custom_data([CustomData(data_type, signal)])
    result = query_catalog(
        catalog,
        NautilusDataType.Custom("PythonCatalogSignal"),
        identifiers=["TEST"],
    )

    assert result["value"].tolist() == [42.5]
    assert result["source"].tolist() == ["python"]
    assert result["ts_event"].astype("int64").tolist() == [11]
    assert result["ts_init"].astype("int64").tolist() == [12]


def test_catalog_to_df_imports_parquet_catalog() -> None:
    """
    Verify catalog to df imports parquet catalog.
    """
    code = """
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import catalog_to_df

assert catalog_to_df._RUST_CATALOG_TYPES == (ParquetDataCatalog,)
"""
    subprocess.run([sys.executable, "-c", code], check=True)


def _value(df, column: str):
    if hasattr(df, "height"):
        return df[column].item()

    value = df[column].iloc[0]
    if value is not None:
        import pandas as pd

        if pd.isna(value):
            return None
    return value


def _timestamp_ns(df, column: str) -> int:
    if hasattr(df, "height"):
        return df[column].dt.timestamp("ns").item()

    return _value(df, column).value


def _make_mark_price():
    return model.MarkPriceUpdate(
        InstrumentId.from_str("ETHUSDT.BINANCE"),
        model.Price.from_str("100.25"),
        50,
        51,
    )


def _make_index_price():
    return model.IndexPriceUpdate(
        InstrumentId.from_str("ETHUSDT.BINANCE"),
        model.Price.from_str("100.25"),
        60,
        61,
    )


def _make_instrument_close():
    return model.InstrumentClose(
        InstrumentId.from_str("ETHUSDT.BINANCE"),
        model.Price.from_str("100.25"),
        model.InstrumentCloseType.CONTRACT_EXPIRED,
        70,
        71,
    )


def _make_instrument_status():
    return model.InstrumentStatus(
        instrument_id=InstrumentId.from_str("ETHUSDT.BINANCE"),
        action=model.MarketStatusAction.TRADING,
        ts_event=80,
        ts_init=81,
        reason="Normal trading",
        trading_event="MARKET_OPEN",
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )


def _make_option_greeks():
    return model.OptionGreeks(
        model.InstrumentId.from_str("BTC-20260529-100000-C.OKX"),
        0.55,
        0.012,
        3.4,
        -1.2,
        0.01,
        0.64,
        None,
        0.66,
        100_000.0,
        None,
        90,
        91,
        model.GreeksConvention.PRICE_ADJUSTED,
    )


def _assert_quotes(df) -> None:
    assert _value(df, "bid_price") == 1987.0
    assert _value(df, "ask_price") == 1988.0
    assert _value(df, "bid_size") == 100_000.0
    assert _value(df, "ask_size") == 100_000.0
    assert _timestamp_ns(df, "ts_init") == 11


def _assert_trades(df) -> None:
    assert _value(df, "price") == 1987.0
    assert _value(df, "size") == 0.1
    assert _timestamp_ns(df, "ts_init") == 21


def _assert_bars(df) -> None:
    assert _value(df, "open") == 1.00002
    assert _value(df, "high") == 1.00004
    assert _value(df, "low") == 1.00001
    assert _value(df, "close") == 1.00003
    assert _value(df, "volume") == 1_000_000.0


def _assert_deltas(df) -> None:
    assert _value(df, "price") == 10000.0
    assert _value(df, "size") == 0.1
    assert _timestamp_ns(df, "ts_init") == 31


def _assert_depths(df) -> None:
    row = df.to_dicts()[0] if hasattr(df, "height") else df.iloc[0].to_dict()
    bids, asks = list(row["bids"]), list(row["asks"])
    assert len(bids) == 10
    assert len(asks) == 10
    assert bids[0] == {"price": 99.0, "size": 100.0, "count": 1, "order_id": 1}
    assert asks[0] == {"price": 100.0, "size": 100.0, "count": 1, "order_id": 11}
    assert _timestamp_ns(df, "ts_init") == 41


def _assert_price_update(df) -> None:
    assert _value(df, "value") == 100.25


def _assert_instrument_close(df) -> None:
    assert _value(df, "close_price") == 100.25
    assert _value(df, "close_type") == "CONTRACT_EXPIRED"
    assert _timestamp_ns(df, "ts_init") == 71


def _assert_instrument_status(df) -> None:
    assert _value(df, "action") == "TRADING"
    assert _value(df, "reason") == "Normal trading"
    assert _value(df, "trading_event") == "MARKET_OPEN"
    assert bool(_value(df, "is_trading")) is True
    assert bool(_value(df, "is_quoting")) is True
    assert bool(_value(df, "is_short_sell_restricted")) is False
    assert _timestamp_ns(df, "ts_init") == 81


def _assert_option_greeks(df) -> None:
    assert _value(df, "delta") == 0.55
    assert _value(df, "gamma") == 0.012
    assert _value(df, "mark_iv") == 0.64
    assert _value(df, "bid_iv") is None
    assert _value(df, "ask_iv") == 0.66
    assert _value(df, "underlying_price") == 100_000.0
    assert _value(df, "open_interest") is None
    assert _value(df, "convention") == "PRICE_ADJUSTED"
    assert _timestamp_ns(df, "ts_init") == 91


@pytest.mark.parametrize("output", [CatalogOutput.POLARS, CatalogOutput.PANDAS])
@pytest.mark.parametrize("catalog_backend", ["parquet"])
@pytest.mark.parametrize(
    ("data_type", "writer_name", "data_factory", "expected_columns", "assert_values"),
    [
        (
            NautilusDataType.QuoteTick,
            "write_quote_ticks",
            lambda: TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11),
            [
                "instrument_id",
                "bid_price",
                "ask_price",
                "bid_size",
                "ask_size",
                "ts_event",
                "ts_init",
            ],
            _assert_quotes,
        ),
        (
            NautilusDataType.TradeTick,
            "write_trade_ticks",
            lambda: TestDataProviderPyo3.trade_tick(ts_event=20, ts_init=21),
            [
                "instrument_id",
                "price",
                "size",
                "aggressor_side",
                "trade_id",
                "ts_event",
                "ts_init",
            ],
            _assert_trades,
        ),
        (
            NautilusDataType.Bar,
            "write_bars",
            TestDataProviderPyo3.bar_5decimal,
            [
                "instrument_id",
                "bar_type",
                "open",
                "high",
                "low",
                "close",
                "volume",
                "ts_event",
                "ts_init",
            ],
            _assert_bars,
        ),
        (
            NautilusDataType.OrderBookDelta,
            "write_order_book_deltas",
            lambda: TestDataProviderPyo3.order_book_delta(ts_event=30, ts_init=31),
            [
                "instrument_id",
                "action",
                "side",
                "price",
                "size",
                "order_id",
                "flags",
                "sequence",
                "ts_event",
                "ts_init",
            ],
            _assert_deltas,
        ),
        (
            NautilusDataType.OrderBookDepth,
            "write_order_book_depths",
            lambda: TestDataProviderPyo3.order_book_depth(ts_event=40, ts_init=41),
            [
                "instrument_id",
                "bids",
                "asks",
                "flags",
                "sequence",
                "ts_event",
                "ts_init",
            ],
            _assert_depths,
        ),
        (
            NautilusDataType.MarkPriceUpdate,
            "write_mark_price_updates",
            _make_mark_price,
            ["instrument_id", "value", "ts_event", "ts_init"],
            _assert_price_update,
        ),
        (
            NautilusDataType.IndexPriceUpdate,
            "write_index_price_updates",
            _make_index_price,
            ["instrument_id", "value", "ts_event", "ts_init"],
            _assert_price_update,
        ),
        (
            NautilusDataType.InstrumentClose,
            "write_instrument_closes",
            _make_instrument_close,
            ["instrument_id", "close_price", "close_type", "ts_event", "ts_init"],
            _assert_instrument_close,
        ),
        (
            NautilusDataType.InstrumentStatus,
            "write_instrument_statuses",
            _make_instrument_status,
            [
                "instrument_id",
                "action",
                "ts_event",
                "ts_init",
                "reason",
                "trading_event",
                "is_trading",
                "is_quoting",
                "is_short_sell_restricted",
            ],
            _assert_instrument_status,
        ),
        (
            NautilusDataType.OptionGreeks,
            "write_option_greeks",
            _make_option_greeks,
            [
                "instrument_id",
                "delta",
                "gamma",
                "vega",
                "theta",
                "rho",
                "mark_iv",
                "bid_iv",
                "ask_iv",
                "underlying_price",
                "open_interest",
                "ts_event",
                "ts_init",
                "convention",
            ],
            _assert_option_greeks,
        ),
    ],
)
def test_query_catalog_dataframe_builtin_data_types(
    tmp_path: Path,
    output: CatalogOutput,
    catalog_backend: str,
    data_type: NautilusDataType,
    writer_name: str,
    data_factory: Callable[[], object],
    expected_columns: list[str],
    assert_values: Callable[[object], None],
) -> None:
    """
    Verify query catalog dataframe builtin data types.
    """
    if output is CatalogOutput.POLARS:
        pytest.importorskip("polars")

    data = data_factory()
    df = _write_and_query(tmp_path, catalog_backend, data_type, writer_name, data, output)

    expected_columns = [*expected_columns, "identifier"]
    assert _columns(df) == expected_columns
    assert _height(df) == 1
    if output is CatalogOutput.PANDAS:
        assert "pyarrow" not in str(df.dtypes["ts_init"])
    assert_values(df)

    identifier_field = "bar_type" if data_type == NautilusDataType.Bar else "instrument_id"
    assert _value(df, "identifier") == str(getattr(data, identifier_field))


def test_query_catalog_defaults_to_pandas(tmp_path: Path) -> None:
    """
    Verify query catalog defaults to pandas.
    """
    df = _write_and_query(
        tmp_path,
        "parquet",
        NautilusDataType.QuoteTick,
        "write_quote_ticks",
        TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11),
    )

    assert type(df).__module__.startswith("pandas")
    assert "pyarrow" not in str(df.dtypes["ts_init"])
    _assert_quotes(df)


def test_query_catalog_accepts_nautilus_data_type(tmp_path: Path) -> None:
    """
    Verify query catalog accepts nautilus data type.
    """
    path = tmp_path / "parquet" / "quotes"
    path.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(path))
    catalog.write_quote_ticks([TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11)])

    df = query_catalog(catalog, NautilusDataType.QuoteTick)

    assert _height(df) == 1
    _assert_quotes(df)


def test_query_catalog_accepts_nautilus_record_type_through_parquet(
    tmp_path: Path,
) -> None:
    """
    Verify query catalog accepts nautilus record type through parquet.
    """
    path = tmp_path / "parquet_catalog"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))
    catalog.write_record_arrow_bytes(
        NautilusRecordType.AccountState,
        _record_batch_arrow_bytes(),
        identifier="AUD/USD.SIM",
    )

    df = query_catalog(
        catalog,
        NautilusRecordType.AccountState,
        identifiers=["AUD/USD.SIM"],
        start=15,
        end=25,
    )

    assert _height(df) == 1
    assert _value(df, "value") == 2


@pytest.mark.parametrize("catalog_backend", ["parquet"])
def test_query_catalog_accepts_nautilus_instrument_type(
    tmp_path: Path,
    catalog_backend: str,
) -> None:
    """
    Verify query catalog accepts nautilus instrument type.
    """
    path = tmp_path / f"{catalog_backend}_catalog"
    if catalog_backend == "parquet":
        path.mkdir()
        catalog = ParquetDataCatalog(str(path))
    else:
        catalog = ParquetDataCatalog(str(path))
    instrument = TestInstrumentProviderPyo3.futures_contract_es()
    other = TestInstrumentProviderPyo3.aapl_equity()
    catalog.write_instruments([instrument, other])

    df = query_catalog(catalog, NautilusInstrumentType.FuturesContract)

    assert _height(df) == 1
    assert _value(df, "instrument_id") == str(instrument.id)
    assert _value(df, "instrument_type") == "FuturesContract"


def test_query_catalog_use_arrow_dtypes_uses_arrow_dtypes(tmp_path: Path) -> None:
    """
    Verify query catalog use arrow dtypes uses arrow dtypes.
    """
    path = tmp_path / "parquet" / "quotes"
    path.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(path))
    catalog.write_quote_ticks(
        [TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11)],
    )
    df = query_catalog(catalog, NautilusDataType.QuoteTick, use_arrow_dtypes=True)

    assert type(df).__module__.startswith("pandas")
    assert "pyarrow" in str(df.dtypes["ts_init"])
    _assert_quotes(df)


def test_query_catalog_rejects_arrow_dtypes_for_non_pandas_output(
    tmp_path: Path,
) -> None:
    """
    Verify query catalog rejects arrow dtypes for non pandas output.
    """
    path = tmp_path / "parquet" / "quotes"
    path.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(path))
    catalog.write_quote_ticks([TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11)])

    with pytest.raises(ValueError, match="only valid with pandas output"):
        query_catalog(
            catalog,
            NautilusDataType.QuoteTick,
            output=CatalogOutput.ARROW,
            use_arrow_dtypes=True,
        )


@pytest.mark.parametrize("output", ["pandas", object()])
def test_query_catalog_rejects_invalid_output(tmp_path: Path, output: object) -> None:
    """
    Verify query catalog rejects invalid output.
    """
    path = tmp_path / "parquet" / "quotes"
    path.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(path))

    with pytest.raises(TypeError, match="output must be a CatalogOutput"):
        query_catalog(
            catalog,
            NautilusDataType.QuoteTick,
            output=output,  # type: ignore[arg-type]
        )


@pytest.mark.parametrize(
    "data_type",
    [
        "quotes",
        model.QuoteTick,
        object(),
    ],
)
@pytest.mark.parametrize("catalog_backend", ["parquet"])
def test_query_catalog_rejects_untyped_data_type(
    tmp_path: Path,
    data_type: object,
    catalog_backend: str,
) -> None:
    """
    Verify query catalog rejects untyped data type.
    """
    path = tmp_path / catalog_backend
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))

    with pytest.raises(
        TypeError,
        match=(
            "data_type must be a NautilusDataType, NautilusRecordType, or NautilusInstrumentType"
        ),
    ):
        query_catalog(catalog, data_type)  # type: ignore[arg-type]


def test_query_catalog_can_return_arrow_table(tmp_path: Path) -> None:
    """
    Verify query catalog can return arrow table.
    """
    table = _write_and_query(
        tmp_path,
        "parquet",
        NautilusDataType.QuoteTick,
        "write_quote_ticks",
        TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11),
        output=CatalogOutput.ARROW,
    )

    assert table.column_names == [
        "instrument_id",
        "bid_price",
        "ask_price",
        "bid_size",
        "ask_size",
        "ts_event",
        "ts_init",
        "identifier",
    ]
    assert table.num_rows == 1


@pytest.mark.parametrize("output", [CatalogOutput.ARROW, CatalogOutput.PANDAS])
def test_arrow_and_pandas_outputs_do_not_import_other_dataframe_engines(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    output: CatalogOutput,
) -> None:
    """
    Verify arrow and pandas outputs do not import other dataframe engines.
    """
    monkeypatch.delitem(sys.modules, "polars", raising=False)
    monkeypatch.delitem(sys.modules, "duckdb", raising=False)

    _write_and_query(
        tmp_path,
        "parquet",
        NautilusDataType.QuoteTick,
        "write_quote_ticks",
        TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11),
        output=output,
    )

    assert "polars" not in sys.modules
    assert "duckdb" not in sys.modules


def test_query_catalog_can_return_duckdb_relation(tmp_path: Path) -> None:
    """
    Verify query catalog can return duckdb relation.
    """
    duckdb = pytest.importorskip("duckdb")
    quote = TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11)

    relation = _write_and_query(
        tmp_path,
        "parquet",
        NautilusDataType.QuoteTick,
        "write_quote_ticks",
        quote,
        output=CatalogOutput.DUCKDB,
    )

    assert isinstance(relation, duckdb.DuckDBPyRelation)
    assert relation.columns == [
        "instrument_id",
        "bid_price",
        "ask_price",
        "bid_size",
        "ask_size",
        "ts_event",
        "ts_init",
        "identifier",
    ]
    assert relation.filter("bid_price = 1987.0").project(
        "instrument_id, bid_price",
    ).fetchall() == [(str(quote.instrument_id), 1987.0)]


def test_arrow_output_supports_caller_owned_duckdb_connection(tmp_path: Path) -> None:
    """
    Verify arrow output supports caller owned duckdb connection.
    """
    duckdb = pytest.importorskip("duckdb")
    quote = TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=11)
    table = _write_and_query(
        tmp_path,
        "parquet",
        NautilusDataType.QuoteTick,
        "write_quote_ticks",
        quote,
        output=CatalogOutput.ARROW,
    )
    connection = duckdb.connect()

    try:
        relation = connection.from_arrow(table)

        assert relation.project("instrument_id, ask_price").fetchall() == [
            (str(quote.instrument_id), 1988.0),
        ]
    finally:
        connection.close()


def _write_custom_and_query(
    tmp_path: Path,
    catalog_backend: str,
    type_name: str,
    data: list[CustomData],
    identifiers: list[str] | None = None,
    output: CatalogOutput = CatalogOutput.PANDAS,
):
    path = tmp_path / catalog_backend / type_name
    path.mkdir(parents=True, exist_ok=True)
    if catalog_backend == "parquet":
        catalog = ParquetDataCatalog(str(path))
        catalog.write_custom_data(data)
        return query_catalog(
            catalog,
            NautilusDataType.Custom(type_name),
            output=output,
            identifiers=identifiers,
        )

    catalog = ParquetDataCatalog(str(path))
    catalog.write_custom_data(data)
    return query_catalog(
        catalog,
        NautilusDataType.Custom(type_name),
        output=output,
        identifiers=identifiers,
    )


@pytest.mark.parametrize("output", [CatalogOutput.POLARS, CatalogOutput.PANDAS])
@pytest.mark.parametrize("catalog_backend", ["parquet"])
def test_query_catalog_dataframe_custom_data(
    tmp_path: Path,
    output: CatalogOutput,
    catalog_backend: str,
) -> None:
    """
    Verify query catalog dataframe custom data.
    """
    if output is CatalogOutput.POLARS:
        pytest.importorskip("polars")

    register_custom_data_class(RustTestCustomData)
    instrument_id = InstrumentId.from_str("RUST.TEST")
    data_type = DataType("RustTestCustomData", {"venue": "TEST"}, str(instrument_id))
    wrapped = [
        CustomData(data_type, RustTestCustomData(instrument_id, 1.23, True, 1, 2)),
    ]

    df = _write_custom_and_query(
        tmp_path,
        catalog_backend,
        "RustTestCustomData",
        wrapped,
        identifiers=[str(instrument_id)],
        output=output,
    )

    expected_columns = [
        "instrument_id",
        "value",
        "flag",
        "ts_event",
        "ts_init",
        "data_type",
        "identifier",
    ]
    assert _columns(df) == expected_columns
    assert _height(df) == 1
    assert _value(df, "instrument_id") == str(instrument_id)
    assert _value(df, "value") == 1.23
    assert bool(_value(df, "flag")) is True
    assert _timestamp_ns(df, "ts_event") == 1
    assert _timestamp_ns(df, "ts_init") == 2
    assert json.loads(_value(df, "data_type"))["type_name"] == "RustTestCustomData"
    assert _value(df, "identifier") == str(instrument_id)


@pytest.mark.parametrize("output", [CatalogOutput.POLARS, CatalogOutput.PANDAS])
@pytest.mark.parametrize("catalog_backend", ["parquet"])
def test_query_catalog_dataframe_custom_data_with_price_map(
    tmp_path: Path,
    output: CatalogOutput,
    catalog_backend: str,
) -> None:
    """
    Verify query catalog dataframe custom data with price map.
    """
    if output is CatalogOutput.POLARS:
        pytest.importorskip("polars")

    register_custom_data_class(RustTestPriceMapCustomData)
    data_type = DataType("RustTestPriceMapCustomData", {"source": "unit-test"}, None)
    prices = {
        InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23456"),
        InstrumentId.from_str("BTCUSDT.BINANCE"): Price.from_str("65432.10"),
    }
    wrapped = [
        CustomData(data_type, RustTestPriceMapCustomData("first", prices, 10, 20)),
    ]

    df = _write_custom_and_query(
        tmp_path,
        catalog_backend,
        "RustTestPriceMapCustomData",
        wrapped,
        output=output,
    )

    expected_columns = [
        "name",
        "prices",
        "ts_event",
        "ts_init",
        "data_type",
        "identifier",
    ]
    assert _columns(df) == expected_columns
    assert _height(df) == 1
    assert _value(df, "name") == "first"
    assert set(json.loads(_value(df, "prices"))) == {"AUD/USD.SIM", "BTCUSDT.BINANCE"}
    assert _timestamp_ns(df, "ts_event") == 10
    assert _timestamp_ns(df, "ts_init") == 20
    assert json.loads(_value(df, "data_type"))["type_name"] == "RustTestPriceMapCustomData"
    assert _value(df, "identifier") is None
