import msgspec


################################################################################
# HTTP responses
################################################################################


class BinanceSpotTradeFee(msgspec.Struct, frozen=True):
    """
    Schema of a single Binance Spot/Margin tradeFee.
    """

    symbol: str
    makerCommission: str
    takerCommission: str
