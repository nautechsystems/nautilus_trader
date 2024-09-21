import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.account.positions import OKXAccountPositionsResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXAccountPositionsGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType | None = None
    instId: str | None = None
    posId: str | None = None

    def validate(self) -> None:
        pass


class OKXAccountPositionsEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/positions"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.ACCOUNT,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXAccountPositionsResponse)

    async def get(self, params: OKXAccountPositionsGetParams) -> OKXAccountPositionsResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
