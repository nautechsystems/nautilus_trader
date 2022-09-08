from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick

from nautilus_trader.core.rust.persistence cimport ParquetType


def py_type_to_parquet_type(cls: type):
    if cls == QuoteTick:
        return ParquetType.QuoteTick
    elif cls == TradeTick:
        return ParquetType.TradeTick
    else:
        raise RuntimeError(f"Type {cls} not supported as a ParquetType yet.")
