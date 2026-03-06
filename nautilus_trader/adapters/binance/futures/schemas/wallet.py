import msgspec


################################################################################
# HTTP responses
################################################################################


class BinanceFuturesCommissionRate(msgspec.Struct, frozen=True):
    """
    Schema of a single Binance Futures commissionRate.
    """

    symbol: str
    makerCommissionRate: str
    takerCommissionRate: str
