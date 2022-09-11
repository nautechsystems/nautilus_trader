# distutils: language = c++
from libcpp.vector cimport vector
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.core.rust.model cimport QuoteTick_t

cdef void* create_vector(list items):
    if isinstance(items[0], QuoteTick):
        return _create_quote_tick_vector(items)

cdef void* _create_quote_tick_vector(list items):
    cdef vector[QuoteTick_t] vec
    [vec.push_back(<QuoteTick_t>(<QuoteTick>item)._mem) for item in items]
    return <void*>vec.data()
    
    

    
