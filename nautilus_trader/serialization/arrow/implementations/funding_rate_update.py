import msgspec
import pyarrow as pa

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(funding_rate: FundingRateUpdate) -> pa.RecordBatch:
    data = funding_rate.to_dict(funding_rate)
    data["rate"] = msgspec.json.encode(data["rate"], enc_hook=msgspec_encoding_hook)
    schema = NAUTILUS_ARROW_SCHEMA[FundingRateUpdate].with_metadata(
        {"instrument_id": funding_rate.instrument_id.value},
    )
    return pa.RecordBatch.from_pylist([data], schema=schema)


def deserialize(batch: pa.RecordBatch) -> list[FundingRateUpdate]:
    def parse(data):
        data["instrument_id"] = batch.schema.metadata[b"instrument_id"].decode()
        data["rate"] = msgspec.json.decode(data["rate"])
        return data

    return [FundingRateUpdate.from_dict(parse(d)) for d in batch.to_pylist()]
