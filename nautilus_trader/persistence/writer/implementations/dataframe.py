import numpy as np
import pandas as pd
import pyarrow as pa

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import FIXED_SCALAR


def float_to_int_dataframe(df: pd.DataFrame) -> pd.DataFrame:
    for name in df.columns:
        if df[name].dtype == float:
            df[name] = df[name].multiply(FIXED_SCALAR).astype("int64")
    return df


def int_to_float_dataframe(df: pd.DataFrame):
    cols = [
        col
        for col, dtype in dict(df.dtypes).items()
        if dtype == np.int64 or dtype == np.uint64 and (col != "ts_event" and col != "ts_init")
    ]
    df[cols] = df[cols] / FIXED_SCALAR
    return df


def quote_tick_dataframe_to_table_rust(df: pd.DataFrame, instrument: Instrument) -> pa.Table:
    metadata = {
        "instrument_id": str(instrument.id),
        "price_precision": str(instrument.price_precision),
        "size_precision": str(instrument.size_precision),
    }
    df = float_to_int_dataframe(df)
    return pa.Table.from_pandas(df).replace_schema_metadata(metadata)
