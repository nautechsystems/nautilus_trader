import msgspec


class BybitServerTime(msgspec.Struct):
    timeSecond: str
    timeNano: str


class BybitServerTimeResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitServerTime
    time: int
