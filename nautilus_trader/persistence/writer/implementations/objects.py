import pandas as pd
import pyarrow as pa

from nautilus_trader.model.data.tick import QuoteTick


def quote_tick_objects_to_table_rust(data: list[QuoteTick]) -> pa.Table:
    df = pd.DataFrame.from_records([QuoteTick.to_raw(x) for x in data])
    metadata = {
        "instrument_id": str(data[0].instrument_id),
        "price_precision": str(data[0].bid.precision),
        "size_precision": str(data[0].bid_size.precision),
    }
    return pa.Table.from_pandas(df).replace_schema_metadata(metadata)
