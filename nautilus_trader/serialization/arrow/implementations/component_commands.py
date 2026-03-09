import pyarrow as pa

from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(command: ShutdownSystem) -> pa.RecordBatch:
    data = command.to_dict(command)
    return pa.RecordBatch.from_pylist([data], schema=NAUTILUS_ARROW_SCHEMA[type(command)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> list[ShutdownSystem]:
        def parse(data):
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner
