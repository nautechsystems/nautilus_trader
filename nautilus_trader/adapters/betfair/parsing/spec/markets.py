from typing import Literal, Optional, Union

import msgspec


class NavigationMarket(msgspec.Struct):
    """NavigationMarket"""

    event_type_name: str
    event_type_id: str
    event_name: Optional[str] = None
    event_id: Optional[str] = None
    event_countryCode: Optional[str] = None
    market_name: str
    market_id: str
    market_exchangeId: str
    market_marketType: str
    market_marketStartTime: str
    market_numberOfWinners: Union[str, int]


class PriceLadderDescription(msgspec.Struct):
    """PriceLadderDescription"""

    type: Literal["CLASSIC", "LINE_RANGE", "FINEST"]


class LineRangeInfo(msgspec.Struct):
    """LineRangeInfo"""

    maxUnitValue: float
    minUnitValue: float
    interval: float
    marketUnit: str


class Description(msgspec.Struct):
    """Description"""

    persistenceEnabled: bool
    bspMarket: bool
    marketTime: str
    suspendTime: str
    bettingType: str
    turnInPlayEnabled: bool
    marketType: str
    regulator: str
    marketBaseRate: float
    discountAllowed: bool
    wallet: str
    rules: str
    rulesHasDate: bool
    lineRangeInfo: Optional[LineRangeInfo] = None
    priceLadderDescription: PriceLadderDescription


class EventType(msgspec.Struct):
    """EventType"""

    id: str
    name: str


class Competition(msgspec.Struct):
    """Competition"""

    id: str
    name: str


class Event(msgspec.Struct):
    """Event"""

    id: str
    name: str
    countryCode: Optional[str] = None
    timezone: str
    openDate: str


class Runner(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    selectionId: int
    sortPriority: Optional[int] = None
    name: Optional[str] = None
    hc: Optional[str] = None
    status: Optional[str] = None
    adjustmentFactor: Optional[float] = None


class MarketCatalog(msgspec.Struct):
    """MarketCatalog"""

    marketId: str
    marketName: str
    marketStartTime: str
    description: Description
    totalMatched: float
    runners: list[Runner]
    eventType: EventType
    competition: Optional[Competition] = None
    event: Event

    @property
    def competition_id(self) -> str:
        if not self.competition:
            return ""
        return self.competition.id

    @property
    def competition_name(self) -> str:
        if not self.competition:
            return ""
        return self.competition.name
