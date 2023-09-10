import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class BybitWalletBalanceHttp(BybitHttpEndpoint):
    def __init__(
        self,
        client: BybitHttpClient,
        base_endpoint: str,
    ):
        self.http_method = HttpMethod.GET
        url_path = base_endpoint + "account/wallet-balance"
        super().__init__(
            client=client,
            endpoint_type=BybitEndpointType.ACCOUNT,
            url_path=url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BybitWalletBalanceResponseStruct)

    async def _get(self):
        raw = await self._method(self.http_method)
        return self._get_resp_decoder.decode(raw)
