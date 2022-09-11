from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick

from nautilus_trader.core.rust.persistence cimport ParquetType


cpdef ParquetType py_type_to_parquet_type(type t):
    if t == QuoteTick:
        return ParquetType.QuoteTick
    elif t == TradeTick:
        return ParquetType.TradeTick
    else:
        raise RuntimeError(f"Type {t} not supported as a ParquetType yet.")
