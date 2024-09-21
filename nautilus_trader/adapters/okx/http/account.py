from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.endpoints.account.balance import OKXAccountBalanceEndpoint
from nautilus_trader.adapters.okx.endpoints.account.balance import OKXAccountBalanceGetParams
from nautilus_trader.adapters.okx.endpoints.account.positions import OKXAccountPositionsEndpoint
from nautilus_trader.adapters.okx.endpoints.account.positions import OKXAccountPositionsGetParams
from nautilus_trader.adapters.okx.endpoints.account.trade_fee import OKXTradeFeeEndpoint
from nautilus_trader.adapters.okx.endpoints.account.trade_fee import OKXTradeFeeGetParams
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.schemas.account.balance import OKXAccountBalanceData
from nautilus_trader.adapters.okx.schemas.account.positions import OKXAccountPositionsResponse
from nautilus_trader.adapters.okx.schemas.account.trade_fee import OKXTradeFee
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.correctness import PyCondition


class OKXAccountHttpAPI:
    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
    ) -> None:
        PyCondition.not_none(client, "client")
        self.client = client
        self._clock = clock
        self.base_endpoint = "/api/v5/account"
        # self.default_settle_coin = "USDT" # TODO: needed?

        self._endpoint_fee_rate = OKXTradeFeeEndpoint(client, self.base_endpoint)
        self._endpoint_balance = OKXAccountBalanceEndpoint(client, self.base_endpoint)
        self._endpoint_positions = OKXAccountPositionsEndpoint(client, self.base_endpoint)

    async def fetch_trade_fee(
        self,
        instType: OKXInstrumentType,
        uly: str | None = None,
        instFamily: str | None = None,
        instId: str | None = None,
    ) -> OKXTradeFee:
        response = await self._endpoint_fee_rate.get(
            OKXTradeFeeGetParams(
                instType=instType,
                uly=uly,
                instFamily=instFamily,
                instId=instId,
            ),
        )
        return response.data[0]

    async def fetch_balance(self, ccy: str | None = None) -> OKXAccountBalanceData:
        response = await self._endpoint_balance.get(
            OKXAccountBalanceGetParams(ccy=ccy),
        )
        return response.data[0]

    async def fetch_positions(
        self,
        instType: OKXInstrumentType | None = None,
        instId: str | None = None,
        posId: str | None = None,
    ) -> OKXAccountPositionsResponse:
        response = await self._endpoint_positions.get(
            OKXAccountPositionsGetParams(instType=instType, instId=instId, posId=posId),
        )
        return response
