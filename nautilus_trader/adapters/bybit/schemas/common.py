from typing import Optional

import msgspec

def BybitListResult(type):
    return msgspec.defstruct("", [("list", list[type])])


def BybitCoinResult(type):
    return msgspec.defstruct("", [("coin", list[type])])


class LeverageFilter(msgspec.Struct):
    # Minimum leverage
    minLeverage: str
    # Maximum leverage
    maxLeverage: str
    # The step to increase/reduce leverage
    leverageStep: str


class LinearPriceFilter(msgspec.Struct):
    # Minimum order price
    minPrice: str
    # Maximum order price
    maxPrice: str
    # The step to increase/reduce order price
    tickSize: str

class SpotPriceFilter(msgspec.Struct):
    tickSize: str


class LotSizeFilter(msgspec.Struct):
    # Maximum order quantity
    maxOrderQty: str
    # Minimum order quantity
    minOrderQty: str
    # The step to increase/reduce order quantity
    qtyStep: str
    # Maximum order qty for PostOnly order
    postOnlyMaxOrderQty: Optional[str] = None

class SpotLotSizeFilter(msgspec.Struct):
    basePrecision: str
    quotePrecision: str
    minOrderQty: str
    maxOrderQty: str
    minOrderAmt: str
    maxOrderAmt: str



class BybitWsSubscriptionMsg(msgspec.Struct):
    success: bool
    op: str
