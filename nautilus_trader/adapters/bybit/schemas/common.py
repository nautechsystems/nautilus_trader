import msgspec


def BybitListResult(type):
    return msgspec.defstruct("", [("list", list[type])])


class LeverageFilter(msgspec.Struct):
    minLeverage: str
    maxLeverage: str
    leverageStep: str


class PriceFilter(msgspec.Struct):
    minPrice: str
    maxPrice: str
    tickSize: str


class LotSizeFilter(msgspec.Struct):
    maxOrderQty: str
    minOrderQty: str
    qtyStep: str
    postOnlyMaxOrderQty: str
