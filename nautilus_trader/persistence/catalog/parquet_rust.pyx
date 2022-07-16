from nautilus_trader.core.rust.catalog cimport Vec_Bar
from nautilus_trader.core.rust.catalog cimport Vec_QuoteTick
from nautilus_trader.core.rust.catalog cimport index_bar_vector
from nautilus_trader.core.rust.catalog cimport index_quote_tick_vector
from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick


cdef list parse_quote_tick_vector(Vec_QuoteTick tick_vec):
    cdef QuoteTick_t _mem
    cdef QuoteTick tick
    cdef list ticks = []
    for i in range(0, tick_vec.len - 1):
        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = index_quote_tick_vector(&tick_vec, i)[0]
        ticks.append(tick)


cdef list parse_bar_vector(Vec_Bar bar_vec):
    cdef Bar_t _mem
    cdef Bar bar
    cdef list bars = []
    for i in range(0, bar_vec.len - 1):
        bar = Bar.__new__(Bar)
        bar.ts_event = _mem.ts_event
        bar.ts_init = _mem.ts_init
        bar._mem = index_bar_vector(&bar_vec, i)[0]
        bars.append(bar)
