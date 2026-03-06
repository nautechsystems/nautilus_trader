from decimal import Decimal

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money


################################################################################
# HTTP responses
################################################################################


class BinanceSpotBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response 'inner struct' from Binance Spot/Margin GET /api/v3/account (HMAC
    SHA256).
    """

    asset: str
    free: str
    locked: str

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        free = Money(Decimal(self.free), currency)
        locked = Money(Decimal(self.locked), currency)
        total = free + locked
        return AccountBalance(
            total=total,
            locked=locked,
            free=free,
        )


class BinanceSpotAccountInfo(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Spot/Margin GET /api/v3/account (HMAC SHA256).
    """

    makerCommission: int
    takerCommission: int
    buyerCommission: int
    sellerCommission: int
    canTrade: bool
    canWithdraw: bool
    canDeposit: bool
    updateTime: int
    accountType: BinanceAccountType
    balances: list[BinanceSpotBalanceInfo]
    permissions: list[str]

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.balances]


class BinanceSpotOrderOco(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Spot/Margin GET /api/v3/orderList (HMAC SHA256).

    HTTP response from Binance Spot/Margin POST /api/v3/order/oco (HMAC SHA256). HTTP
    response from Binance Spot/Margin DELETE /api/v3/orderList (HMAC SHA256).

    """

    orderListId: int
    contingencyType: str
    listStatusType: str
    listOrderStatus: str
    listClientOrderId: str
    transactionTime: int
    symbol: str
    orders: list[BinanceOrder] | None = None  # Included for ACK response type
    orderReports: list[BinanceOrder] | None = None  # Included for FULL & RESPONSE types
