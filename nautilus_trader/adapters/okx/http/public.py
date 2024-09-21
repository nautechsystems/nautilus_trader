from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.endpoints.public.instruments import OKXInstrumentsEndpoint
from nautilus_trader.adapters.okx.endpoints.public.instruments import OKXInstrumentsGetParams
from nautilus_trader.adapters.okx.endpoints.public.position_tiers import OKXPositionTiersEndpoint
from nautilus_trader.adapters.okx.endpoints.public.position_tiers import OKXPositionTiersGetParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrument
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentList
from nautilus_trader.adapters.okx.schemas.public.position_tiers import OKXPositionTiersResponse
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXPublicHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/public"

        self._endpoint_instruments = OKXInstrumentsEndpoint(client, self.base_endpoint)
        self._endpoint_position_tiers = OKXPositionTiersEndpoint(client, self.base_endpoint)

    def _get_url(self, url: str) -> str:
        return self.base_endpoint + url

    async def fetch_instruments(
        self,
        instType: OKXInstrumentType,
        ctType: OKXContractType | None = None,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> OKXInstrumentList:
        response = await self._endpoint_instruments.get(
            OKXInstrumentsGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        if ctType:
            return [i for i in response.data if i.ctType == ctType]  # type: ignore
        return response.data

    async def fetch_instrument(
        self,
        instType: OKXInstrumentType,
        instId: str,
        uly: str | None = None,
        instFamily: str | None = None,
    ) -> OKXInstrument:
        response = await self._endpoint_instruments.get(
            OKXInstrumentsGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        return response.data[0]

    async def fetch_position_tiers(
        self,
        instType: OKXInstrumentType,
        tdMode: OKXTradeMode,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
        ccy: str | None = None,
        tier: str | None = None,
    ) -> OKXPositionTiersResponse:
        response = await self._endpoint_position_tiers.get(
            OKXPositionTiersGetParams(
                instType=instType,
                tdMode=tdMode,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
                ccy=ccy,
                tier=tier,
            ),
        )
        return response
