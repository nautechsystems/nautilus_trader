from collections.abc import Callable

import pyarrow as pa

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA_RUST
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists


def quote_ticks_to_record_batch(data: list[QuoteTick]) -> pa.RecordBatch:
    schema = NAUTILUS_PARQUET_SCHEMA_RUST[QuoteTick]
    raw = [QuoteTick.to_raw(q) for q in data]

    # Munge into format required by record_batch (list of values_
    munged = list(list_dicts_to_dict_lists(raw, keys=schema.names).values())
    metadata = {
        "instrument_id": str(data[0].instrument_id),
        "price_precision": str(data[0].bid.precision),
        "size_precision": str(data[0].bid_size.precision),
    }
    return pa.record_batch(munged, schema=schema.with_metadata(metadata))


RECORD_BATCH_SERIALIZERS: dict[type, Callable] = {
    QuoteTick: quote_ticks_to_record_batch,
}


# TODO - Use explosion.ai library for registering callables?
def register_record_batch_serializer(cls: type, serializer: Callable):
    RECORD_BATCH_SERIALIZERS[cls] = serializer
