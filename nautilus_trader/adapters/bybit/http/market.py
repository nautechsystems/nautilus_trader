import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.endpoints.market.instruments_info import BybitInstrumentsInfoEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod

from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrument
from nautilus_trader.common.clock import LiveClock

from nautilus_trader.adapters.bybit.utils import get_category_from_account_type


class BybitMarketHttpAPI:
    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        account_type: BybitAccountType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/v5/market/"
        self.account_type = account_type

        # endpoints
        self._endpoint_instruments = BybitInstrumentsInfoEndpoint(
            client=client,
            base_endpoint=self.base_endpoint,
            account_type=account_type
        )

    def _get_url(self, url: str):
        return self.base_endpoint + url

    async def fetch_instruments(self)->list[BybitInstrument]:
        response = await self._endpoint_instruments._get(
            parameters=self._endpoint_instruments.GetParameters(
                category=get_category_from_account_type(self.account_type)
            )
        )
        return response.result.list

    async def get_risk_limits(self):
        params = {"category": "linear"}
        try:
            raw: bytes = await self.client.send_request(
                http_method=HttpMethod.GET,
                url_path=self._get_url("risk-limit"),
                payload=params,
            )
            decoded = self._decoder_risk_limit.decode(raw)
            return decoded.result.list
        except Exception as e:
            print(e)



