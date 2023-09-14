import msgspec


class BybitServerTime(msgspec.Struct):
    timeSecond: str
    timeNano: str


class BybitServerTimeResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitServerTime
    time: int
