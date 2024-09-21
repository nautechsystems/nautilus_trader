from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_aggressor_side(side: str | OKXOrderSide) -> AggressorSide:
    match side:
        case "buy":
            return AggressorSide.BUYER
        case OKXOrderSide.BUY:
            return AggressorSide.BUYER
        case "sell":
            return AggressorSide.SELLER
        case OKXOrderSide.SELL:
            return AggressorSide.SELLER
        case _:
            raise ValueError(f"Invalid aggressor side, was '{side}'")


def parse_okx_ws_delta(  #  for websocket "books5-l2-tbt" channel
    instrument_id: InstrumentId,
    values: tuple[Price, Quantity],  # either bid values or ask values
    side: OrderSide,
    sequence: int,
    ts_event: int,
    ts_init: int,
    is_snapshot: bool,
    flags: int = 0,
) -> OrderBookDelta:
    price = values[0]
    size = values[1]
    if is_snapshot:
        action = BookAction.ADD
    else:
        action = BookAction.DELETE if size == 0 else BookAction.UPDATE

    return OrderBookDelta(
        instrument_id=instrument_id,
        action=action,
        order=BookOrder(
            side=side,
            price=price,
            size=size,
            order_id=0,
        ),
        flags=flags,
        sequence=sequence,
        ts_event=ts_event,
        ts_init=ts_init,
    )
