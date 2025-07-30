from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import OrderBook
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity

BETFAIR_FLOAT_TO_PRICE: dict[float, Price]
BETFAIR_PRICE_PRECISION: int
BETFAIR_QUANTITY_PRECISION: int

def create_betfair_order_book(instrument_id: InstrumentId) -> OrderBook: ...
def betfair_float_to_price(value: float) -> Price: ...
def betfair_float_to_quantity(value: float) -> Quantity: ...