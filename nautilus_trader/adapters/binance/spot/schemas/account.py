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

    @property
    def can_trade(self) -> bool:
        return self.canTrade

    @property
    def event_time_ms(self) -> int | None:
        return self.updateTime

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.balances]


class BinanceMarginBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response inner struct from Binance Cross Margin `GET /sapi/v1/margin/account`.
    """

    asset: str
    borrowed: str
    free: str
    interest: str
    locked: str
    netAsset: str

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        total = Money(Decimal(self.netAsset), currency)
        locked = Money(Decimal(self.locked), currency)
        free = Money(Decimal(self.netAsset) - Decimal(self.locked), currency)
        return AccountBalance(
            total=total,
            locked=locked,
            free=free,
        )


class BinanceMarginAccountInfo(msgspec.Struct, frozen=True):
    """
    HTTP response from Binance Cross Margin `GET /sapi/v1/margin/account`.
    """

    borrowEnabled: bool
    marginLevel: str
    totalAssetOfBtc: str
    totalLiabilityOfBtc: str
    totalNetAssetOfBtc: str
    tradeEnabled: bool
    transferInEnabled: bool
    transferOutEnabled: bool
    userAssets: list[BinanceMarginBalanceInfo]

    @property
    def can_trade(self) -> bool:
        return self.tradeEnabled

    @property
    def event_time_ms(self) -> int | None:
        return None

    def parse_to_account_balances(self) -> list[AccountBalance]:
        return [balance.parse_to_account_balance() for balance in self.userAssets]


class BinancePortfolioMarginBalanceInfo(msgspec.Struct, frozen=True):
    """
    HTTP response inner struct from Binance Portfolio Margin `GET /papi/v1/balance`.
    """

    asset: str
    totalWalletBalance: str
    crossMarginAsset: str | None = None
    crossMarginBorrowed: str | None = None
    crossMarginFree: str | None = None
    crossMarginInterest: str | None = None
    crossMarginLocked: str | None = None
    updateTime: int | None = None

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.asset)
        total_value = Decimal(
            self.crossMarginAsset
            if self.crossMarginAsset is not None
            else self.totalWalletBalance
        )
        locked_value = Decimal(self.crossMarginLocked or "0")
        total = Money(total_value, currency)
        locked = Money(locked_value, currency)
        return AccountBalance(
            total=total,
            locked=locked,
            free=Money(total_value - locked_value, currency),
        )


class BinancePortfolioMarginAccountInfo(msgspec.Struct, kw_only=True, frozen=True):
    """
    Minimal Portfolio Margin account snapshot assembled from `GET /papi/v1/balance`.
    """

    balances: list[BinancePortfolioMarginBalanceInfo]
    canTrade: bool = True
    updateTime: int | None = None

    @property
    def can_trade(self) -> bool:
        return self.canTrade

    @property
    def event_time_ms(self) -> int | None:
        return self.updateTime

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
