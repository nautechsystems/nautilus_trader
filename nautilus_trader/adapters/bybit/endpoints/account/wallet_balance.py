import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEndpointType
from nautilus_trader.adapters.bybit.endpoints.endpoint import BybitHttpEndpoint
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitWalletBalanceResponse
from nautilus_trader.core.nautilus_pyo3.network import HttpMethod


class WalletBalanceGetaParameters(msgspec.Struct, omit_defaults=True, frozen=False):
    accountType: str = None
    coin: str = None


class BybitWalletBalanceEndpoint(BybitHttpEndpoint):
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
        self._get_resp_decoder = msgspec.json.Decoder(BybitWalletBalanceResponse)

    async def _get(self, parameters: WalletBalanceGetaParameters) -> BybitWalletBalanceResponse:
        raw = await self._method(self.http_method, parameters)
        return self._get_resp_decoder.decode(raw)
