from nautilus_trader.adapters.betfair.common import BETFAIR_FLOAT_TO_PRICE
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.core.rust.model import BookType

from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cpdef inline OrderBook create_betfair_order_book(InstrumentId instrument_id):
    return OrderBook(
        instrument_id,
        BookType.L2_MBP,
    )


cpdef Price betfair_float_to_price(double value):
    try:
        return BETFAIR_FLOAT_TO_PRICE[value]
    except KeyError:
        return Price(value, BETFAIR_PRICE_PRECISION)


cpdef Quantity betfair_float_to_quantity(double value):
    return Quantity(value, BETFAIR_QUANTITY_PRECISION)
