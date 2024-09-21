import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXCancelOrderResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXCancelOrderPostParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instId: str
    ordId: str | None = None
    clOrdId: str | None = None

    def validate(self) -> None:
        assert (
            self.ordId or self.clOrdId
        ), "either `ordId` or `clOrdId` is required to cancel an order"


class OKXCancelOrderEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/cancel-order"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXCancelOrderResponse)

    async def post(self, params: OKXCancelOrderPostParams) -> OKXCancelOrderResponse:
        # Validate
        params.validate()

        method_type = HttpMethod.POST
        raw = await self._method(method_type, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
