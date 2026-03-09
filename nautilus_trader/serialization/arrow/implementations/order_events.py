import msgspec
import pyarrow as pa

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(event: OrderInitialized | OrderFilled) -> pa.RecordBatch:
    data = event.to_dict(event)
    if isinstance(event, OrderInitialized):
        data["options"] = msgspec.json.encode(data["options"], enc_hook=msgspec_encoding_hook)
        data["linked_order_ids"] = msgspec.json.encode(
            data["linked_order_ids"],
            enc_hook=msgspec_encoding_hook,
        )
        data["exec_algorithm_params"] = msgspec.json.encode(
            data["exec_algorithm_params"],
            enc_hook=msgspec_encoding_hook,
        )
        data["tags"] = msgspec.json.encode(data["tags"], enc_hook=msgspec_encoding_hook)
    elif isinstance(event, OrderFilled):
        data["info"] = msgspec.json.encode(data["info"], enc_hook=msgspec_encoding_hook)
    return pa.RecordBatch.from_pylist([data], schema=NAUTILUS_ARROW_SCHEMA[type(event)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> OrderInitialized | OrderFilled:
        def parse(data):
            if cls == OrderInitialized:
                data["options"] = msgspec.json.decode(data["options"])
                data["linked_order_ids"] = msgspec.json.decode(data["linked_order_ids"])
                data["exec_algorithm_params"] = msgspec.json.decode(data["exec_algorithm_params"])
                data["tags"] = msgspec.json.decode(data["tags"])
            elif cls == OrderFilled:
                data["info"] = msgspec.json.decode(data["info"])
            else:
                raise RuntimeError("Unsupported order event type for deserialization")
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner
