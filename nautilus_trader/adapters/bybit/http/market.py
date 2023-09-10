import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.market import BybitInstrumentsResponseStruct
from nautilus_trader.adapters.bybit.schemas.market import BybitRiskLimitResponseStruct
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitMarketHttpAPI(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        account_type: BybitAccountType,
    ):
        PyCondition.not_none(client, "client")
        super().__init__(client, {}, "test")
        self.client = client
        self.base_endpoint = "/v5/market/"
        self.account_type = account_type

        # decoders
        self._decoder_instruments = msgspec.json.Decoder(BybitInstrumentsResponseStruct)
        self._decoder_risk_limit = msgspec.json.Decoder(BybitRiskLimitResponseStruct)

    def _get_url(self, url: str):
        return self.base_endpoint + url

    async def get_instruments_info(self):
        params = {"category": "linear"}
        try:
            raw: bytes = await self.client.send_request(
                http_method=HttpMethod.GET,
                url_path=self._get_url("instruments-info"),
                payload=params,
            )
            decoded = self._decoder_instruments.decode(raw)
            return decoded.result.list
        except Exception as e:
            print(e)

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
