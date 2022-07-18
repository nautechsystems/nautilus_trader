from cpython.object cimport PyObject
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.core.rust.model cimport pyobject_to_option_parse_test
from nautilus_trader.core.rust.model cimport instrument_id_to_pystr
from nautilus_trader.core.rust.model cimport create_price_option
from nautilus_trader.core.rust.model cimport Option_InstrumentId
from nautilus_trader.core.rust.model cimport Option_Price
from nautilus_trader.core.rust.model cimport create_instrument_id_option

cdef test_pyobject_to_option_parse():
    # Test PyObject > Option<T> with value
    cdef InstrumentId instrument_id
    instrument_id = InstrumentId.from_str("EUR/USD.DUKA")
    pyobject_to_option_parse_test(<PyObject *>instrument_id)

    # Test PyObject > Option<T> with None
    instrument_id = None
    pyobject_to_option_parse_test(<PyObject *>instrument_id)

cdef test_option_access_from_cython():
    # Test access to Option<Price> from Cython
    cdef Option_Price price_option = create_price_option()
    print(price_option)
    print(price_option.some)
    print(price_option.some.raw, price_option.some.precision)


    # Test access to Option<InstrumentId> from Cython
    
    # Can't print InstrumentId_t because it contains a string.
    # ERROR: Cannot convert 'InstrumentId_t' to Python object
    cdef Option_InstrumentId instrument_id_option = create_instrument_id_option()
    print(instrument_id_option.tag)
    #print(instrument_id_option) # error
    #print(instrument_id_option.some) # error
    instrument_id_option.some
    instrument_id_option.some.symbol
    instrument_id_option.some.venue

cpdef main():
    test_pyobject_to_option_parse()
    test_option_access_from_cython()

    
  
      

