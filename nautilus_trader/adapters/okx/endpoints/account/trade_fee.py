import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.account.trade_fee import OKXTradeFeeResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXTradeFeeGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType
    uly: str | None = None
    instFamily: str | None = None
    instId: str | None = None

    def validate(self) -> None:
        if self.instType == OKXInstrumentType.OPTION:
            assert (
                self.uly or self.instFamily
            ), "`uly` or `instFamily` is required for OPTION type instruments"


class OKXTradeFeeEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/trade-fee"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.ACCOUNT,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXTradeFeeResponse)

    async def get(self, params: OKXTradeFeeGetParams) -> OKXTradeFeeResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
