import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.core.data import Data
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.persistence.writer.implementations import dataframe_to_table
from nautilus_trader.persistence.writer.implementations import objects_to_table
from nautilus_trader.persistence.writer.implementations.dataframe import (
    quote_tick_dataframe_to_table_rust,
)
from nautilus_trader.persistence.writer.implementations.objects import (
    quote_tick_objects_to_table_rust,
)
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA_RUST


def dataframe_to_table(
    df: pd.DataFrame,
    cls: type,
    use_rust: bool = True,
    kwargs: dict = None,
) -> pa.Table:
    """
    kwargs: Additional keyword-arguments required for the conversion
    """
    if cls is QuoteTick and use_rust:
        assert "instrument" in kwargs
        table = quote_tick_dataframe_to_table_rust(df, instrument=kwargs["instrument"])
    else:
        raise NotImplementedError()

    assert table is not None
    return table


def objects_to_table(data: list[Data], use_rust: bool = True) -> pa.Table:
    assert len(data) > 0
    cls = type(data[0])
    assert all(isinstance(obj, Data) for obj in data)  # same type
    assert all(type(obj) is cls for obj in data)  # same type

    if cls is QuoteTick and use_rust:
        assert all(x.instrument_id == data[0].instrument_id for x in data)  # same instrument_id
        table = quote_tick_objects_to_table_rust(data)
    else:
        raise NotImplementedError()

    assert table is not None
    return table


class ParquetWriter:
    def __init__(
        self,
        fs: fsspec.filesystem = fsspec.filesystem("file"),
        use_rust=True,
    ):
        self._fs = fs
        self._use_rust = use_rust

    def write_objects(self, data: list[Data], path: str) -> None:
        """Write nautilus_objects to a ParquetFile"""
        assert len(data) > 0
        cls = type(data[0])
        table = objects_to_table(data, use_rust=self._use_rust)
        self._write(table, path=path, cls=cls)

    def write_dataframe(self, df: pd.DataFrame, path: str, cls: type, **kwargs) -> None:
        """
        Write a dataframe containing nautilus object data to a ParquetFile
        kwargs: additional kwargs needed to perform the table conversion
        """
        # TODO cast integer columns to the schema if safe
        # TODO convert from float to int dataframe format
        table = dataframe_to_table(df, use_rust=self._use_rust, kwargs=kwargs)
        self._write(table, path=path, cls=cls)

    def _write(self, table: pa.Table, path: str, cls: type) -> None:
        if self._use_rust and cls in (QuoteTick, TradeTick):
            expected_schema = NAUTILUS_PARQUET_SCHEMA_RUST.get(cls)
        else:
            expected_schema = NAUTILUS_PARQUET_SCHEMA.get(cls)

        if expected_schema is None:
            raise RuntimeError(f"Schema not found for class {cls}")

        # Check columns exists
        for name in expected_schema.names:
            assert (
                name in table.schema.names
            ), f"Invalid schema for table: {name} column not found in table columns {table.schema.names}"

        # Drop unused columns
        table = table.select(expected_schema.names)

        # Assert table schema
        assert table.schema == expected_schema

        # Write parquet file
        self._fs.makedirs(self._fs._parent(str(path)), exist_ok=True)
        with pq.ParquetWriter(path, table.schema) as writer:
            writer.write_table(table)
