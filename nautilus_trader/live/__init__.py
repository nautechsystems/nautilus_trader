"""
The `live` subpackage groups all engine and client implementations for live trading.

Generally a common event loop is passed into each live engine to support the overarching
design of a single efficient event loop, by default
`uvloop <https://github.com/MagicStack/uvloop>`_.

"""
