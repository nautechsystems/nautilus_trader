import msgspec

from nautilus_trader.adapters.bybit.schemas.common import BybitCoinResult, BybitListResult


class BybitCoinBalance(msgspec.Struct):
    availableToBorrow: str
    bonus: str
    accruedInterest: str
    availableToWithdraw: str
    totalOrderIM: str
    equity: str
    totalPositionMM: str
    usdValue: str
    unrealisedPnl: str
    collateralSwitch: bool
    borrowAmount: str
    totalPositionIM: str
    walletBalance: str
    cumRealisedPnl: str
    locked: str
    marginCollateral: bool
    coin: str


class BybitWalletBalance(msgspec.Struct):
    totalEquity: str
    accountIMRate: str
    totalMarginBalance: str
    totalInitialMargin: str
    accountType: str
    totalAvailableBalance: str
    accountMMRate: str
    totalPerpUPL: str
    totalWalletBalance: str
    accountLTV: str
    totalMaintenanceMargin: str
    coin: list[BybitCoinBalance]



class BybitWalletBalanceResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult(BybitWalletBalance)
    time: int