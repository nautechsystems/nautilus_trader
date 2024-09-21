from nautilus_trader.adapters.okx.endpoints.market.books import OKXBooksEndpoint
from nautilus_trader.adapters.okx.endpoints.market.books import OKXBooksGetParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.market.books import OKXOrderBookSnapshotResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXMarketHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/market"

        self._endpoint_books = OKXBooksEndpoint(client, self.base_endpoint)

    async def fetch_books(self, instId: str, depth: str) -> OKXOrderBookSnapshotResponse:
        response = await self._endpoint_books.get(
            OKXBooksGetParams(instId=instId, sz=depth),
        )
        return response
