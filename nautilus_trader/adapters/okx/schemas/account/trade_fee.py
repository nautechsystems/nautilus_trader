import msgspec as msgspec

from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType


class OKXTradeFee(msgspec.Struct):
    category: str  # deprecated
    delivery: str  # Delivery fee rate
    exercise: str  # Fee rate for exercising the option
    instType: OKXInstrumentType
    level: str  # Fee rate Level
    maker: str  # maker fee rate
    makerU: str  # maker for USDT-margined contracts
    makerUSDC: str  # maker for USDC-margined instruments
    taker: str  # taker fee rate
    takerU: str  # taker for USDT-margined instruments
    takerUSDC: str  # taker for USDC-margined instruments
    ts: str  # Unix timestamp in milliseconds
    fiat: list  # Details of fiat fee rate?


class OKXTradeFeeResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXTradeFee]
