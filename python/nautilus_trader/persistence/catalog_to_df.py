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
DataFrame conversion for queries through Rust-backed Nautilus catalogs.
"""

from __future__ import annotations

from enum import Enum
from enum import unique
from typing import Any

import pyarrow as pa

from nautilus_trader import model
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.persistence import ParquetDataCatalog


type CatalogQueryType = (
    model.NautilusDataType | model.NautilusRecordType | model.NautilusInstrumentType
)

type Catalog = ParquetDataCatalog

_INSTRUMENT_DATA_TYPE = model.NautilusDataType.Instrument
_RUST_CATALOG_TYPES = (ParquetDataCatalog,)


@unique
class CatalogOutput(Enum):
    ARROW = "arrow"
    PANDAS = "pandas"
    POLARS = "polars"
    DUCKDB = "duckdb"


class ArrowCStream:
    def __init__(self, capsule: object) -> None:
        self._capsule = capsule

    def __arrow_c_stream__(self, requested_schema: object | None = None) -> object:
        if self._capsule is None:
            raise RuntimeError("Arrow C stream has already been consumed")

        capsule = self._capsule
        self._capsule = None
        return capsule


def query_catalog(
    catalog: Catalog,
    data_type: CatalogQueryType,
    output: CatalogOutput = CatalogOutput.PANDAS,
    identifiers: list[str] | None = None,
    start: Any | None = None,
    end: Any | None = None,
    where: str | None = None,
    use_arrow_dtypes: bool = False,
    as_of: Any | None = None,
    display: bool = True,
) -> Any:
    """
    Query a Rust catalog and convert the display-friendly Arrow result.

    Parameters
    ----------
    catalog : ParquetDataCatalog
        The Rust-backed catalog instance to query.
    data_type : NautilusDataType, NautilusRecordType, or NautilusInstrumentType
        The typed Nautilus catalog family to query.
    output : CatalogOutput, default CatalogOutput.PANDAS
        The Python result representation. Polars and DuckDB are imported lazily.
    identifiers : list[str], optional
        The identifiers to filter the query. Record queries accept at most one identifier.
    start : object, optional
        The inclusive query start timestamp.
    end : object, optional
        The inclusive query end timestamp.
    where : str, optional
        The SQL predicate to apply while scanning catalog data.
    use_arrow_dtypes : bool, default False
        Whether pandas output should use Arrow-backed dtypes. This is invalid for other outputs.
    as_of : int or datetime, optional
        Reserved for catalogs supporting historical queries. Parquet requires ``None``.
    display : bool, default True
        Whether to convert batches to the display view (named ``Float64`` columns at each
        instrument's precision, internal columns dropped). Pass ``False`` for the raw open
        catalog format: exact ``Decimal128(38, 16)`` price and quantity columns and all
        storage columns. Instrument queries do not support ``False``.

    Returns
    -------
    Any
        A PyArrow table, pandas DataFrame, Polars DataFrame, or DuckDB relation.

    Notes
    -----
    Multi-identity data results order identity groups lexically and preserve input order within
    each group. They do not promise global chronological order.

    Display conversion requires current-format Arrow data. Migrate legacy catalogs first.
    Depth display uses nested ``bids`` and ``asks`` lists with ``price``, ``size``,
    ``count``, and ``order_id`` fields for every level. Empty sides are empty lists.
    Display prices and sizes are floating-point values; order IDs remain exact integers.

    With ``display=False`` and pandas output, decimal columns convert to ``decimal.Decimal``
    objects unless ``use_arrow_dtypes`` is enabled.

    """
    _require_rust_catalog(catalog)
    _require_catalog_query_type(data_type)
    _require_catalog_output(output)

    if use_arrow_dtypes and output is not CatalogOutput.PANDAS:
        raise ValueError("use_arrow_dtypes is only valid with pandas output")
    if as_of is not None:
        raise ValueError("as_of is not supported by ParquetDataCatalog")

    start_nanos = _timestamp_to_nanos(start)
    end_nanos = _timestamp_to_nanos(end)

    if isinstance(data_type, model.NautilusRecordType):
        table = _query_record_arrow_table(
            catalog,
            data_type,
            identifiers,
            start_nanos,
            end_nanos,
            where,
            as_of,
            display,
        )
    elif isinstance(data_type, model.NautilusInstrumentType) or data_type == _INSTRUMENT_DATA_TYPE:
        if as_of is not None:
            raise ValueError("as_of is not supported for instrument queries")
        if not display:
            raise ValueError("display=False is not supported for instrument queries")
        table = _query_instrument_arrow_table(
            catalog,
            data_type if isinstance(data_type, model.NautilusInstrumentType) else None,
            identifiers,
            start_nanos,
            end_nanos,
            where,
        )
    else:
        table = _query_data_arrow_table(
            catalog,
            data_type,
            identifiers,
            start_nanos,
            end_nanos,
            where,
            as_of,
            display,
        )

    return _convert_arrow_table(
        table,
        output=output,
        use_arrow_dtypes=use_arrow_dtypes,
    )


def _require_rust_catalog(catalog: Catalog) -> None:
    if not isinstance(catalog, _RUST_CATALOG_TYPES):
        raise TypeError(
            "catalog must be a ParquetDataCatalog",
        )


def _require_catalog_query_type(data_type: object) -> None:
    if not isinstance(
        data_type,
        (
            model.NautilusDataType,
            model.NautilusRecordType,
            model.NautilusInstrumentType,
        ),
    ):
        raise TypeError(
            "data_type must be a NautilusDataType, NautilusRecordType, or NautilusInstrumentType",
        )


def _require_catalog_output(output: object) -> None:
    if not isinstance(output, CatalogOutput):
        raise TypeError("output must be a CatalogOutput")


def _query_data_arrow_table(
    catalog: Catalog,
    data_type: model.NautilusDataType,
    identifiers: list[str] | None,
    start: int | None,
    end: int | None,
    where: str | None,
    as_of: Any | None,
    display: bool,
) -> pa.Table:
    stream = catalog.query_data_arrow_stream(
        data_type,
        identifiers,
        start,
        end,
        where,
        display=display,
        as_of=as_of,
    )
    return _arrow_table_from_stream(stream)


def _query_record_arrow_table(
    catalog: Catalog,
    record_type: model.NautilusRecordType,
    identifiers: list[str] | None,
    start: int | None,
    end: int | None,
    where: str | None,
    as_of: Any | None,
    display: bool,
) -> pa.Table:
    if identifiers is not None and len(identifiers) > 1:
        raise ValueError("record catalog queries support at most one identifier")

    identifier = identifiers[0] if identifiers else None

    stream = catalog.query_record_arrow_stream(
        record_type,
        identifier,
        start,
        end,
        where,
        display=display,
        as_of=as_of,
    )
    return _arrow_table_from_stream(stream)


def _query_instrument_arrow_table(
    catalog: Catalog,
    instrument_type: model.NautilusInstrumentType | None,
    identifiers: list[str] | None,
    start: int | None,
    end: int | None,
    where: str | None,
) -> pa.Table:
    stream = catalog.query_instrument_arrow_stream(
        identifiers,
        start,
        end,
        where,
        instrument_type,
    )
    return _arrow_table_from_stream(stream)


def _timestamp_to_nanos(value: Any | None) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool):
        raise TypeError("timestamp bounds must not be bool")
    if isinstance(value, int):
        return value
    return dt_to_unix_nanos(value)


def _arrow_table_from_stream(capsule: object) -> pa.Table:
    reader = pa.RecordBatchReader.from_stream(ArrowCStream(capsule))
    return reader.read_all()


def _convert_arrow_table(
    table: pa.Table,
    output: CatalogOutput,
    use_arrow_dtypes: bool,
) -> Any:
    if output is CatalogOutput.ARROW:
        return table

    if output is CatalogOutput.POLARS:
        import polars as pl

        return pl.from_arrow(table)

    if output is CatalogOutput.DUCKDB:
        import duckdb

        return duckdb.from_arrow(table)

    if output is CatalogOutput.PANDAS:
        import pandas as pd

        if use_arrow_dtypes:
            return table.to_pandas(types_mapper=pd.ArrowDtype)

        return table.to_pandas()

    raise RuntimeError(f"Unhandled catalog output: {output!r}")
