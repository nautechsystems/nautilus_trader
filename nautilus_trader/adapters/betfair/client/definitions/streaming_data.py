from typing import List, Literal, Optional

import msgspec


class RunnerValues(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    tv: float  # Traded Volume
    ltp: float  # Last Traded Price
    spn: float  # Starting Price Near
    spf: float  # Starting Price Far


class Runner(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    adjustmentFactor: float
    status: str
    sortPriority: int
    id: str


class MarketDefinition(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    bspMarket: bool
    turnInPlayEnabled: bool
    persistenceEnabled: bool
    marketBaseRate: int
    eventId: str
    eventTypeId: str
    numberOfWinners: int
    bettingType: str
    marketType: str
    marketTime: str
    suspendTime: str
    bspReconciled: bool
    complete: bool
    inPlay: bool
    crossMatching: bool
    runnersVoidable: bool
    numberOfActiveRunners: int
    betDelay: int
    status: str
    runners: List[Runner]
    regulators: List[str]
    venue: str
    countryCode: str
    discountAllowed: bool
    timezone: str
    openDate: str
    version: int


class RunnerChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    # TODO - Can we type batb etc better?
    batb: Optional[List[List]] = None  # Best Available To Back
    batl: Optional[List[List]] = None  # Best Available To Lay
    bdatb: Optional[List[List]] = None  # Best Display Available To Back  (virtual)
    bdatl: Optional[List[List]] = None  # Best Display Available To Lay (virtual)
    ltp: int
    tv: int
    id: str


class MarketChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: str
    marketDefinition: MarketDefinition
    rc: List[RunnerChange]
    img: bool
    tv: float


class MarketChangeMessage(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    op: Literal["connection", "status", "mcm", "ocm"]
    id: int
    initialClk: Optional[str] = None
    clk: str
    conflateMs: Optional[int] = None
    heartbeatMs: Optional[int] = None
    pt: int
    ct: str
    mc: List[MarketChange] = []
