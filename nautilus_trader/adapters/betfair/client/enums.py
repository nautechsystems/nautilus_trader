from enum import Enum


class MarketProjection(Enum):
    """
    Represents a `Betfair` market projection type.
    """

    COMPETITION = "COMPETITION"
    EVENT = "EVENT"
    EVENT_TYPE = "EVENT_TYPE"
    MARKET_START_TIME = "MARKET_START_TIME"
    MARKET_DESCRIPTION = "MARKET_DESCRIPTION"
    RUNNER_DESCRIPTION = "RUNNER_DESCRIPTION"
    RUNNER_METADATA = "RUNNER_METADATA"


class MarketSort(Enum):
    """
    Represents a `Betfair` market sort type.
    """

    MINIMUM_TRADED = "MINIMUM_TRADED"
    MAXIMUM_TRADED = "MAXIMUM_TRADED"
    MINIMUM_AVAILABLE = "MINIMUM_AVAILABLE"
    MAXIMUM_AVAILABLE = "MAXIMUM_AVAILABLE"
    FIRST_TO_START = "FIRST_TO_START"
    LAST_TO_START = "LAST_TO_START"


class MarketBettingType(Enum):
    """
    Represents a `Betfair` market betting type.
    """

    ODDS = "ODDS"
    LINE = "LINE"
    RANGE = "RANGE"
    ASIAN_HANDICAP_DOUBLE_LINE = "ASIAN_HANDICAP_DOUBLE_LINE"
    ASIAN_HANDICAP_SINGLE_LINE = "ASIAN_HANDICAP_SINGLE_LINE"
    FIXED_ODDS = "FIXED_ODDS"


class OrderStatus(Enum):
    """
    Represents a `Betfair` order status.
    """

    PENDING = "PENDING"
    EXECUTION_COMPLETE = "EXECUTION_COMPLETE"
    EXECUTABLE = "EXECUTABLE"
    EXPIRED = "EXPIRED"


class OrderProjection(Enum):
    """
    Represents a `Betfair` order projection.
    """

    ALL = "ALL"
    EXECUTABLE = "EXECUTABLE"
    EXECUTION_COMPLETE = "EXECUTION_COMPLETE"
