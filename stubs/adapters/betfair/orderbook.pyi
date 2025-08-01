from nautilus_trader.core.rust.model import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.objects import OrderBook


def create_betfair_order_book(instrument_id: InstrumentId) -> OrderBook: ...
def betfair_float_to_price(value: float) -> Price: ...
def betfair_float_to_quantity(value: float) -> Quantity: ...
