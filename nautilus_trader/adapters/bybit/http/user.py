from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.core.correctness import PyCondition


class BybitUserHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
