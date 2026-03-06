from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cpdef OrderBook create_betfair_order_book(InstrumentId instrument_id)
cpdef Price betfair_float_to_price(double value)
cpdef Quantity betfair_float_to_quantity(double value)
