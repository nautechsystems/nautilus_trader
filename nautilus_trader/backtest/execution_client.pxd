from nautilus_trader.backtest.engine cimport SimulatedExchange
from nautilus_trader.execution.client cimport ExecutionClient


cdef class BacktestExecClient(ExecutionClient):
    cdef SimulatedExchange _exchange
