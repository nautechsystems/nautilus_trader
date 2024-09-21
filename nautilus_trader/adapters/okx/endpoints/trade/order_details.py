import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXOrderDetailsResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXOrderDetailsGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str
    ordId: str | None = None  # either ordId or clOrdId is required
    clOrdId: str | None = None  # either ordId or clOrdId is required

    def validate(self) -> None:
        assert (
            self.ordId or self.clOrdId
        ), "either `ordId` or `clOrdId` is required to get order details"


class OKXOrderDetailsEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/order"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXOrderDetailsResponse)

    async def get(self, params: OKXOrderDetailsGetParams) -> OKXOrderDetailsResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
