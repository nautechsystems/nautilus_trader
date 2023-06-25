import fsspec
import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.core.data import Data
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA


def objects_to_table(data: list[Data]) -> pa.Table:
    assert len(data) > 0
    cls = type(data[0])
    assert issubclass(cls, Data)
    assert all(type(obj) is cls for obj in data)  # same type

    # serializer = RECORD_BATCH_SERIALIZERS.get(cls)
    # if serializer is None:
    #     raise KeyError(
    #         f"Not serializer registered for type={cls}, register in {RECORD_BATCH_SERIALIZERS.__module__}",
    #     )

    # TODO
    def serializer(x):
        return data

    batch = serializer(data)
    assert batch is not None
    return pa.Table.from_batches([batch])


class ParquetWriter:
    def __init__(
        self,
        fs: fsspec.filesystem = fsspec.filesystem("file"),
    ):
        self._fs = fs

    def write_objects(self, data: list[Data], path: str) -> None:
        """
        Write nautilus_objects to a ParquetFile.
        """
        assert len(data) > 0
        cls = type(data[0])
        table = objects_to_table(data)
        self._write(table, path=path, cls=cls)

    def _write(self, table: pa.Table, path: str, cls: type) -> None:
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
        with pq.ParquetWriter(path, table.schema, version="2.6") as writer:
            writer.write_table(table)
