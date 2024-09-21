import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEndpointType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXTransactionType
from nautilus_trader.adapters.okx.endpoints.endpoint import OKXHttpEndpoint
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.trade import OKXFillsResponse
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class OKXFillsGetParams(msgspec.Struct, omit_defaults=True, frozen=True):
    instType: OKXInstrumentType | None = None
    uly: str | None = None
    instFamily: str | None = None
    instId: str | None = None
    ordId: str | None = None
    subType: OKXTransactionType | None = None  # Transaction type (int to str)
    after: str | None = None  # Pagination of data for records older than requested billId
    before: str | None = None  # Pagination of data for records newer than requested billId
    begin: str | None = None
    end: str | None = None
    limit: str | None = None  # 100 is max

    def validate(self) -> None:
        if self.instType == OKXInstrumentType.OPTION:
            assert (
                self.uly or self.instFamily
            ), "`uly` or `instFamily` is required for OPTION type instruments"


class OKXFillsEndpoint(OKXHttpEndpoint):
    def __init__(
        self,
        client: OKXHttpClient,
        base_endpoint: str,
    ) -> None:
        url_path = base_endpoint + "/fills"
        super().__init__(
            client=client,
            endpoint_type=OKXEndpointType.TRADE,
            url_path=url_path,
        )
        self._resp_decoder = msgspec.json.Decoder(OKXFillsResponse)

    async def get(self, params: OKXFillsGetParams) -> OKXFillsResponse:
        # Validate
        params.validate()

        raw = await self._method(HttpMethod.GET, params)  # , ratelimiter_keys=[self.url_path])
        try:
            return self._resp_decoder.decode(raw)
        except Exception as e:
            raise RuntimeError(
                f"Failed to decode response from {self.url_path}: {raw.decode()} from error: {e}",
            )
