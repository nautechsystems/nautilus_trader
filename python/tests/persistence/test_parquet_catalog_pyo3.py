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
from decimal import Decimal
from pathlib import Path
from typing import Any

import duckdb
import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from tests.providers import TestInstrumentProvider
from tests.stubs import TestDataProviderPyo3

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import NautilusRecordType
from nautilus_trader.model import Price
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.persistence import RustTestPriceMapCustomData
from nautilus_trader.persistence import StreamingFeatherWriter
from nautilus_trader.persistence.catalog_to_df import ArrowCStream


def _catalog(tmp_path: Path, backend: str) -> Any:
    path = tmp_path / backend
    path.mkdir()
    return ParquetDataCatalog(str(path))


def _read_arrow_bytes(data: bytes) -> pa.Table:
    return pa.ipc.open_stream(pa.py_buffer(data)).read_all()


def _record_batch_bytes(batch: pa.RecordBatch) -> bytes:
    sink = pa.BufferOutputStream()
    with pa.ipc.new_stream(sink, batch.schema) as writer:
        writer.write_batch(batch)
    return sink.getvalue().to_pybytes()


def _read_arrow_stream(capsule: object) -> pa.Table:
    return pa.RecordBatchReader.from_stream(ArrowCStream(capsule)).read_all()


def _instrument(ts_init: int, venue_extra: str, count: int, enabled: bool) -> CurrencyPair:
    base = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    payload = {
        **base.to_dict(),
        "info": {"venue_extra": venue_extra, "count": count, "enabled": enabled},
        "ts_event": ts_init,
        "ts_init": ts_init,
    }
    return CurrencyPair.from_dict(payload)


def test_migration_planner_resolves_funding_and_close_files(tmp_path: Path) -> None:
    """
    Verify migration planner resolves funding and close files.
    """
    source_path = tmp_path / "source"
    target_path = tmp_path / "target"
    target_path.mkdir()
    instrument_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    values = {
        "funding_rate_update": FundingRateUpdate(
            instrument_id,
            Decimal("0.000123456789012345"),
            1_699_999_000_000_000_000,
            1_699_999_100_000_000_000,
            interval=480,
            next_funding_ns=1_700_000_000_000_000_000,
        ),
        "instrument_close": InstrumentClose(
            instrument_id,
            Price.from_str("100.25"),
            InstrumentCloseType.CONTRACT_EXPIRED,
            123,
            124,
        ),
    }

    for type_name, value in values.items():
        staging = tmp_path / "staging" / type_name
        staging.mkdir(parents=True)
        writer = StreamingFeatherWriter(
            path=str(staging),
            cache=Cache(),
            clock=Clock.new_test(),
        )
        writer.write(value)
        writer.close()
        [feather_path] = staging.rglob("*.feather")
        directory = source_path / "data" / type_name
        directory.mkdir(parents=True)
        table = _read_arrow_bytes(feather_path.read_bytes())
        pq.write_table(table, directory / "python.parquet")

    target = ParquetDataCatalog(str(target_path))

    assert target.migrate_from_legacy_parquet_path(str(source_path), dry_run=True) == 0
    assert target.migrate_from_legacy_parquet_path(str(source_path)) == 2


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("transport", ["bytes", "stream"])
def test_raw_multi_identity_query_uses_truthful_ipc_schema(
    tmp_path: Path,
    backend: str,
    transport: str,
) -> None:
    """
    Verify raw multi identity query uses truthful ipc schema.
    """
    catalog = _catalog(tmp_path, backend)
    first_id = InstrumentId(Symbol("PRECISION-A"), Venue("TEST"))
    second_id = InstrumentId(Symbol("PRECISION-B"), Venue("TEST"))
    first = TestDataProviderPyo3.quote_tick(
        instrument_id=first_id,
        bid_price=1.23,
        ask_price=1.24,
        ts_event=1,
        ts_init=1,
    )
    second = TestDataProviderPyo3.quote_tick(
        instrument_id=second_id,
        bid_price=10.12345,
        ask_price=10.12346,
        ts_event=2,
        ts_init=2,
    )
    catalog.write_quote_ticks([second])
    catalog.write_quote_ticks([first])
    query_args = (
        NautilusDataType.QuoteTick,
        [str(first_id), str(second_id)],
        None,
        None,
        None,
        False,
    )
    table = (
        _read_arrow_bytes(catalog.query_data_arrow_bytes(*query_args))
        if transport == "bytes"
        else _read_arrow_stream(catalog.query_data_arrow_stream(*query_args))
    )

    assert table.schema.metadata in (None, {})
    assert table.schema.field("bid_price").type == pa.decimal128(38, 16)
    assert table.schema.field("ask_price").type == pa.decimal128(38, 16)
    assert table.schema.field("bid_size").type == pa.decimal128(38, 16)
    assert table.schema.field("ask_size").type == pa.decimal128(38, 16)
    assert table.schema.field("identifier").type == pa.string()
    assert table.column("bid_price").to_pylist() == [Decimal("1.23"), Decimal("10.12345")]
    assert table.column("ask_price").to_pylist() == [Decimal("1.24"), Decimal("10.12346")]
    assert table.column("identifier").to_pylist() == [str(first_id), str(second_id)]
    assert table.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert table.column("ts_init").cast(pa.int64()).to_pylist() == [1, 2]


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("transport", ["bytes", "stream"])
def test_raw_multi_identity_status_query_preserves_identifiers(
    tmp_path: Path,
    backend: str,
    transport: str,
) -> None:
    """
    Verify raw multi identity status query preserves identifiers.
    """
    catalog = _catalog(tmp_path, backend)
    first_id = InstrumentId.from_str("STATUS-A.TEST")
    second_id = InstrumentId.from_str("STATUS-B.TEST")
    first = InstrumentStatus(
        first_id,
        MarketStatusAction.TRADING,
        1,
        1,
        reason="Normal trading",
        trading_event="MARKET_OPEN",
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )
    second = InstrumentStatus(
        second_id,
        MarketStatusAction.HALT,
        2,
        2,
        reason="Trading halt",
        trading_event="MARKET_HALT",
        is_trading=False,
        is_quoting=False,
        is_short_sell_restricted=True,
    )
    catalog.write_instrument_statuses([second])
    catalog.write_instrument_statuses([first])
    query_args = (
        NautilusDataType.InstrumentStatus,
        [str(first_id), str(second_id)],
        None,
        None,
        None,
        False,
    )
    table = (
        _read_arrow_bytes(catalog.query_data_arrow_bytes(*query_args))
        if transport == "bytes"
        else _read_arrow_stream(catalog.query_data_arrow_stream(*query_args))
    )

    assert table.schema.metadata in (None, {})
    assert table.column("instrument_id").to_pylist() == [str(first_id), str(second_id)]
    assert table.column("identifier").to_pylist() == [str(first_id), str(second_id)]
    assert table.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert table.column("ts_init").cast(pa.int64()).to_pylist() == [1, 2]


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("transport", ["bytes", "stream"])
@pytest.mark.parametrize("display", [False, True])
def test_empty_record_query_uses_canonical_schema(
    tmp_path: Path,
    backend: str,
    transport: str,
    display: bool,
) -> None:
    """
    Verify empty record query uses canonical schema.
    """
    catalog = _catalog(tmp_path, backend)
    table = (
        _read_arrow_bytes(
            catalog.query_record_arrow_bytes(NautilusRecordType.AccountState, display=display),
        )
        if transport == "bytes"
        else _read_arrow_stream(
            catalog.query_record_arrow_stream(NautilusRecordType.AccountState, display=display),
        )
    )

    assert table.num_rows == 0
    assert table.column_names == [
        "account_id",
        "account_type",
        "base_currency",
        "balances",
        "margins",
        "is_reported",
        "event_id",
        "ts_event",
        "ts_init",
        "info",
    ]


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("display", [False, True])
def test_empty_unregistered_custom_query_reports_schema_error(
    tmp_path: Path,
    backend: str,
    display: bool,
) -> None:
    """
    Verify empty unregistered custom query reports schema error.
    """
    catalog = _catalog(tmp_path, backend)

    with pytest.raises(OSError, match="UnregisteredCatalogQuery"):
        catalog.query_data_arrow_bytes(
            NautilusDataType.Custom("UnregisteredCatalogQuery"),
            display=display,
        )


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("display", [False, True])
def test_query_data_arrow_bytes_empty_schema_matches_nonempty(
    tmp_path: Path,
    backend: str,
    display: bool,
) -> None:
    """
    Verify query data arrow bytes empty schema matches nonempty.
    """
    catalog = _catalog(tmp_path, backend)
    quote = TestDataProviderPyo3.quote_tick(ts_event=10, ts_init=10)
    catalog.write_quote_ticks([quote])
    nonempty = _read_arrow_bytes(
        catalog.query_data_arrow_bytes(NautilusDataType.QuoteTick, display=display),
    )
    empty = _read_arrow_bytes(
        catalog.query_data_arrow_bytes(NautilusDataType.QuoteTick, start=11, display=display),
    )

    assert empty.schema == nonempty.schema
    assert empty.num_rows == 0
    if not display:
        assert empty.schema.field("bid_price").type == pa.decimal128(38, 16)
        assert empty.schema.field("ask_price").type == pa.decimal128(38, 16)
        assert empty.schema.field("bid_size").type == pa.decimal128(38, 16)
        assert empty.schema.field("ask_size").type == pa.decimal128(38, 16)


@pytest.mark.parametrize("backend", ["parquet"])
@pytest.mark.parametrize("display", [False, True])
def test_custom_query_data_arrow_bytes_empty_schema_matches_nonempty(
    tmp_path: Path,
    backend: str,
    display: bool,
) -> None:
    """
    Verify custom query data arrow bytes empty schema matches nonempty.
    """
    register_custom_data_class(RustTestCustomData)
    catalog = _catalog(tmp_path, backend)
    instrument_id = InstrumentId.from_str("CUSTOM-SCHEMA.TEST")
    identifier = str(instrument_id)
    data_type = DataType("RustTestCustomData", {"source": "schema-test"}, identifier)
    custom = CustomData(data_type, RustTestCustomData(instrument_id, 1.25, True, 10, 20))
    catalog.write_custom_data([custom])
    nonempty = _read_arrow_bytes(
        catalog.query_data_arrow_bytes(
            NautilusDataType.Custom("RustTestCustomData"),
            identifiers=[identifier],
            display=display,
        ),
    )
    empty = _read_arrow_bytes(
        catalog.query_data_arrow_bytes(
            NautilusDataType.Custom("RustTestCustomData"),
            identifiers=[identifier],
            start=21,
            display=display,
        ),
    )

    assert empty.schema == nonempty.schema
    assert empty.num_rows == 0
    assert empty.schema.field("identifier").type == pa.string()
    assert empty.schema.field("instrument_id").type == pa.string()


def test_query_data_arrow_bytes_rejects_as_of(tmp_path: Path) -> None:
    """
    Verify query data arrow bytes rejects as of.
    """
    path = tmp_path / "parquet"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))

    with pytest.raises(ValueError, match="ParquetDataCatalog does not support as_of"):
        catalog.query_data_arrow_bytes(NautilusDataType.QuoteTick, as_of=0)


def test_query_data_arrow_bytes_rejects_string_selector(tmp_path: Path) -> None:
    """
    Verify query data arrow bytes rejects string selector.
    """
    path = tmp_path / "parquet"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))

    with pytest.raises(TypeError, match="data_type must be NautilusDataType"):
        catalog.query_data_arrow_bytes("QuoteTick")  # type: ignore[arg-type]


def test_catalog_instruments_applies_where_clause(tmp_path: Path) -> None:
    """
    Verify catalog instruments applies where clause.
    """
    path = tmp_path / "parquet"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))
    catalog.write_instruments([_instrument(1_000, "v1", 1, True)])
    catalog.write_instruments([_instrument(2_000, "v2", 2, False)])

    filtered = catalog.instruments(
        instrument_ids=["AUD/USD.SIM"],
        where_clause="ts_init = TIMESTAMP '1970-01-01 00:00:00.000002'",
    )

    assert len(filtered) == 1
    assert filtered[0].ts_init == 2_000
    assert filtered[0].info == {"venue_extra": "v2", "count": 2, "enabled": False}


def test_pyo3_parquet_catalog_query_metadata_returns_dict(tmp_path: Path) -> None:
    """
    Verify pyo3 parquet catalog query metadata returns dict.
    """
    path = tmp_path / "parquet"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))
    first = TestDataProviderPyo3.quote_tick(
        bid_price=1.1,
        ask_price=1.2,
        ts_event=10,
        ts_init=10,
    )
    second = TestDataProviderPyo3.quote_tick(
        bid_price=1.1,
        ask_price=1.2,
        ts_event=20,
        ts_init=20,
    )
    catalog.write_quote_ticks([first, second])

    metadata = catalog.query_metadata(
        NautilusDataType.QuoteTick,
        [str(first.instrument_id)],
        start=20,
        end=20,
    )

    assert list(metadata) == [20]
    assert metadata[20]["instrument_id"] == str(first.instrument_id)
    assert metadata[20]["price_precision"] == "1"


def test_parquet_catalog_files_are_open_to_external_arrow_and_json_readers(
    tmp_path: Path,
) -> None:
    """
    Verify parquet catalog files are open to external arrow and json readers.
    """
    path = tmp_path / "parquet"
    path.mkdir()
    catalog = ParquetDataCatalog(str(path))
    timestamp = 1_715_212_800_000_000_123
    quote = TestDataProviderPyo3.quote_tick(ts_event=timestamp, ts_init=timestamp)
    instrument = _instrument(timestamp, "external", 7, True)
    register_custom_data_class(RustTestCustomData)
    register_custom_data_class(RustTestPriceMapCustomData)
    custom_instrument_id = InstrumentId.from_str("EXTERNAL.TEST")
    custom = CustomData(
        DataType("RustTestCustomData", None, str(custom_instrument_id)),
        RustTestCustomData(custom_instrument_id, 1.25, True, timestamp + 1, timestamp + 1),
    )
    price_map = CustomData(
        DataType("RustTestPriceMapCustomData", None, None),
        RustTestPriceMapCustomData(
            "external-prices",
            {InstrumentId.from_str("AUD/USD.SIM"): Price.from_str("1.23456")},
            timestamp + 2,
            timestamp + 2,
        ),
    )
    account_schema = _read_arrow_bytes(
        catalog.query_record_arrow_bytes(
            NautilusRecordType.AccountState,
            display=False,
        ),
    ).schema
    account_state = pa.RecordBatch.from_pylist(
        [
            {
                "account_id": "EXTERNAL-ACCOUNT",
                "account_type": "CASH",
                "base_currency": "USD",
                "balances": '[{"currency":"USD","total":"100.00"}]',
                "margins": "[]",
                "is_reported": True,
                "event_id": "00000000-0000-0000-0000-000000000001",
                "ts_event": timestamp + 3,
                "ts_init": timestamp + 3,
            },
        ],
        schema=account_schema,
    )
    catalog.write_quote_ticks([quote])
    catalog.write_instruments([instrument])
    catalog.write_custom_data([custom])
    catalog.write_custom_data([price_map])
    catalog.write_record_arrow_bytes(
        NautilusRecordType.AccountState,
        _record_batch_bytes(account_state),
        identifier="EXTERNAL-ACCOUNT",
    )

    files = [pq.ParquetFile(file_path) for file_path in path.rglob("*.parquet")]
    tables = [file.read() for file in files]
    quote_table = next(table for table in tables if "bid_price" in table.column_names)
    instrument_table = next(
        table
        for table in tables
        if "info" in table.column_names and "raw_symbol" in table.column_names
    )
    custom_table = next(table for table in tables if {"value", "flag"} <= set(table.column_names))
    price_map_table = next(table for table in tables if "prices" in table.column_names)
    account_table = next(table for table in tables if "balances" in table.column_names)
    instrument_file = next(
        file
        for file in files
        if "info" in file.schema_arrow.names and "raw_symbol" in file.schema_arrow.names
    )
    price_map_file = next(file for file in files if "prices" in file.schema_arrow.names)
    account_file = next(file for file in files if "balances" in file.schema_arrow.names)
    quote_file = next(file for file in files if "bid_price" in file.schema_arrow.names)
    bid_price_index = quote_file.schema_arrow.get_field_index("bid_price")
    bid_price_stats = quote_file.metadata.row_group(0).column(bid_price_index).statistics

    assert quote_table.schema.field("bid_price").type == pa.decimal128(38, 16)
    assert quote_table.schema.field("bid_size").type == pa.decimal128(38, 16)
    assert quote_table.schema.field("ts_event").type == pa.timestamp("ns", tz="UTC")
    assert quote_table.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert custom_table.schema.field("ts_event").type == pa.timestamp("ns", tz="UTC")
    assert custom_table.schema.field("ts_init").type == pa.timestamp("ns", tz="UTC")
    assert instrument_table.schema.field("info").type == pa.json_()
    assert price_map_table.schema.field("prices").type == pa.json_()
    assert account_table.schema.field("balances").type == pa.json_()
    info_index = instrument_file.schema_arrow.get_field_index("info")
    prices_index = price_map_file.schema_arrow.get_field_index("prices")
    balances_index = account_file.schema_arrow.get_field_index("balances")
    ts_init_index = quote_file.schema_arrow.get_field_index("ts_init")
    assert instrument_file.schema.column(info_index).logical_type.type == "JSON"
    assert price_map_file.schema.column(prices_index).logical_type.type == "JSON"
    assert account_file.schema.column(balances_index).logical_type.type == "JSON"
    assert quote_file.schema.column(ts_init_index).logical_type.type == "TIMESTAMP"
    assert quote_table.column("bid_price")[0].as_py() == Decimal(1987)
    assert bid_price_stats.min == Decimal(1987)
    assert bid_price_stats.max == Decimal(1987)
    assert json.loads(instrument_table.column("info")[0].as_py()) == {
        "venue_extra": "external",
        "count": 7,
        "enabled": True,
    }
    assert json.loads(price_map_table.column("prices")[0].as_py()) == {
        "AUD/USD.SIM": "1.23456",
    }

    with duckdb.connect() as connection:
        connection.execute("SET TimeZone = 'UTC'")
        connection.register("instruments", instrument_table)
        instrument_info = connection.execute(
            """
            SELECT
                json_extract_string(info, '$.venue_extra'),
                CAST(json_extract(info, '$.count') AS INTEGER),
                CAST(json_extract(info, '$.enabled') AS BOOLEAN)
            FROM instruments
            WHERE json_extract_string(info, '$.venue_extra') = 'external'
            """,
        ).fetchall()

        connection.register("price_maps", price_map_table)
        connection.register("account_states", account_table)
        prices = connection.execute(
            "SELECT json_extract_string(prices, '$.\"AUD/USD.SIM\"') FROM price_maps",
        ).fetchall()
        balances = connection.execute(
            """
            SELECT
                json_extract_string(balances, '$[0].currency'),
                json_extract_string(balances, '$[0].total')
            FROM account_states
            """,
        ).fetchall()

        connection.register("quotes", quote_table)
        connection.register("custom_data", custom_table)
        quote_timestamp = connection.execute(
            """
            SELECT epoch_ns(ts_init) FROM quotes
            WHERE ts_init >= TIMESTAMP '2024-05-09'
              AND ts_init < TIMESTAMP '2024-05-10'
            """,
        ).fetchone()
        custom_timestamp = connection.execute(
            """
            SELECT epoch_ns(ts_init) FROM custom_data
            WHERE ts_init >= TIMESTAMP '2024-05-09'
              AND ts_init < TIMESTAMP '2024-05-10'
            """,
        ).fetchone()

    assert instrument_info == [("external", 7, True)]
    assert prices == [("1.23456",)]
    assert balances == [("USD", "100.00")]
    assert quote_table.column("ts_init").cast(pa.int64())[0].as_py() == timestamp
    assert quote_timestamp == (timestamp // 1_000 * 1_000,)
    assert custom_table.column("ts_init").cast(pa.int64())[0].as_py() == timestamp + 1
    assert custom_timestamp == ((timestamp + 1) // 1_000 * 1_000,)
