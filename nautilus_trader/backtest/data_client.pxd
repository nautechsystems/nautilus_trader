from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument


cdef class BacktestDataClient(DataClient):
    pass


cdef class BacktestMarketDataClient(MarketDataClient):
    cdef bint _has_futures(self, list components)
    cdef Instrument _create_option_spread_from_components(self, InstrumentId spread_instrument_id, list spread_legs)
    cdef Instrument _create_futures_spread_from_components(self, InstrumentId spread_instrument_id, list spread_legs)
