"""
The `data` subpackage groups components relating to the data stack and data tooling for
the platform.

The layered architecture of the data stack somewhat mirrors the
execution stack with a central engine, cache layer beneath, database layer
beneath, with alternative implementations able to be written on top.

Due to the high-performance, the core components are reusable between both
backtest and live implementations - helping to ensure consistent logic for
trading operations.

"""
