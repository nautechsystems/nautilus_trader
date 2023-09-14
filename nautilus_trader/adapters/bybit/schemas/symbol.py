from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType


class BybitSymbol(str):
    def __new__(cls, symbol: Optional[str]):
        if symbol is not None:
            return super().__new__(
                cls,
                symbol.upper().replace(" ", "").replace("/", "").replace("-PERP", ""),
            )

    def parse_as_nautilus(self, instrument_type: BybitInstrumentType) -> str:
        if instrument_type.is_spot_or_margin:
            return str(self)

        if self[-1].isdigit():
            return str(self)
        if self.endswith("_PERP"):
            return str(self).replace("_", "-")
        else:
            return str(self) + "-PERP"
