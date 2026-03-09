"""
The `execution` subpackage groups components relating to the execution stack for the
platform.

The layered architecture of the execution stack somewhat mirrors the
data stack with a central engine, cache layer beneath, database layer
beneath, with alternative implementations able to be written on top.

Due to the high-performance, the core components are reusable between both
backtest and live implementations - helping to ensure consistent logic for
trading operations.

"""
