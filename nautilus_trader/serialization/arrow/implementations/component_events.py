import msgspec
import pyarrow as pa

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(event: ComponentStateChanged | TradingStateChanged) -> pa.RecordBatch:
    data = event.to_dict(event)
    data["config"] = msgspec.json.encode(data["config"], enc_hook=msgspec_encoding_hook)
    return pa.RecordBatch.from_pylist([data], schema=NAUTILUS_ARROW_SCHEMA[type(event)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> list[ComponentStateChanged | TradingStateChanged]:
        def parse(data):
            data["config"] = msgspec.json.decode(data["config"])
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner
