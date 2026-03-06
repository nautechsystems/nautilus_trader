import msgspec


class BinanceListenKey(msgspec.Struct):
    """
    HTTP response from creating a new Binance user listen key.
    """

    listenKey: str
