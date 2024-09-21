from typing import Literal

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXOrderStatus
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXOrderDetailsResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXOrdersPendingGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType
    uly: str | None = None
    instFamily: str | None = None
    instId: str | None = None
    ordType: OKXOrderType | None = None
    state: OKXOrderStatus | None = None  # live or partially_filled
    category: (
        Literal["twap", "adl", "full_liquidation", "partial_liquidation", "delivery", "ddh"] | None
    ) = None
    after: str | None = None  # pagination for records older than the requested ordId
    before: str | None = None  # pagination for records newer than the requested ordId
    begin: str | None = None
    end: str | None = None
    limit: str | None = None  # 100 is max

    def validate(self) -> None:
        if self.instType == OKXInstrumentType.OPTION:
            assert (
                self.uly or self.instFamily
            ), "`uly` or `instFamily` is required for OPTION type instruments"


class OKXOrdersPendingEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/orders-pending"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXOrderDetailsResponse)

    async def get(self, params: OKXOrdersPendingGetParams) -> OKXOrderDetailsResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
