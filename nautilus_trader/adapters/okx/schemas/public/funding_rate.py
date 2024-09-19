from typing import Literal

import msgspec


class OKXFundingRateHistoryData(msgspec.Struct):
    instType: str
    instId: str
    method: Literal["current_period", "next_period"]
    fundingRate: str  # Predicted funding rate
    realizedRate: str  # Actual funding rate
    fundingTime: str  # settlement time


class OKXFundingRateHistoryResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXFundingRateHistoryData]


class OKXFundingRateData(msgspec.Struct):
    instType: str
    instId: str
    method: Literal["current_period", "next_period"]
    fundingRate: str  # Current funding rate
    nextFundingRate: str  # Forecasted funding rate for next period; "" if method is current_period
    fundingTime: str
    nextFundingTime: str
    minFundingRate: str
    maxFundingRate: str
    settState: str
    settFundingRate: str
    premium: str
    ts: str


class OKXFundingRateResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXFundingRateData]
