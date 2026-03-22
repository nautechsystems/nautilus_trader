import enum

class CandleDirection(enum.IntEnum):
    DIRECTION_BEAR = -1
    DIRECTION_NONE = 0
    DIRECTION_BULL = 1

class CandleSize(enum.IntEnum):
    SIZE_NONE = 0
    SIZE_VERY_SMALL = 1
    SIZE_SMALL = 2
    SIZE_MEDIUM = 3
    SIZE_LARGE = 4
    SIZE_VERY_LARGE = 5
    SIZE_EXTREMELY_LARGE = 6

class CandleBodySize(enum.IntEnum):
    BODY_NONE = 0
    BODY_SMALL = 1
    BODY_MEDIUM = 2
    BODY_LARGE = 3
    BODY_TREND = 4

class CandleWickSize(enum.IntEnum):
    WICK_NONE = 0
    WICK_SMALL = 1
    WICK_MEDIUM = 2
    WICK_LARGE = 3
