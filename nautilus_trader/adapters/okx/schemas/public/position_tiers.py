import msgspec


class OKXPositionTiersData(msgspec.Struct):
    uly: str
    instFamily: str
    instId: str
    tier: str
    minSz: str
    maxSz: str
    mmr: str
    imr: str
    maxLever: str
    optMgnFactor: str
    quoteMaxLoan: str
    baseMaxLoan: str


class OKXPositionTiersResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXPositionTiersData]
