import msgspec


class BinanceListenKey(msgspec.Struct):
    """
    HTTP response from creating a new Binance user listen key.
    """

    listenKey: str


class BinanceListenToken(msgspec.Struct):
    """
    HTTP response from creating a new Binance margin user listen token.
    """

    token: str
    expirationTime: int
