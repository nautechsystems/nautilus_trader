import hashlib

import orjson

from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeId


MKT_DEPTH_OPERATIONS = {
    0: BookAction.ADD,
    1: BookAction.UPDATE,
    2: BookAction.DELETE,
}

IB_SIDE = {1: OrderSide.BUY, 0: OrderSide.SELL}

# TODO
IB_TICK_TYPE = {
    1: "Last",
    2: "AllLast",
    3: "BidAsk",
    4: "MidPoint",
}


def generate_trade_id(symbol: str, ts_event: int, price: str, size: str) -> TradeId:
    hash_values = (symbol, ts_event, price, size)
    h = hashlib.sha256(orjson.dumps(hash_values))
    return TradeId(h.hexdigest())
