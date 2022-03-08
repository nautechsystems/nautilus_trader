from typing import List, Literal, Optional

import msgspec


# class MarketSubscription(msgspec.Struct):
#     {"op": "marketSubscription", "id": 2,
#      "marketFilter": {"marketIds": ["1.120684740"], "bspMarket": true, "bettingTypes": ["ODDS"], "eventTypeIds": ["1"],
#                       "eventIds": ["27540841"], "turnInPlayEnabled": true, "marketTypes": ["MATCH_ODDS"],
#                       "countryCodes": ["ES"]}, "marketDataFilter": {}}


class ConnectionMessage(msgspec.Struct):
    """
    Connection Message
    """

    op: Literal["connection"]
    connectionId: str


class StatusMessage(msgspec.Struct):
    """
    Status Message
    """

    op: Literal["status"]
    statusCode: Literal["SUCCESS", "FAILURE"]
    connectionClosed: bool
    errorCode: str
    errorMessage: str
    connectionsAvailable: int


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
    hc: Optional[str] = None


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
    hc: Optional[float] = None


class MarketChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: str
    marketDefinition: MarketDefinition
    rc: List[RunnerChange]
    img: bool
    tv: float
    con: Optional[bool] = None


class MarketChangeMessage(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    op: Literal["connection", "status", "mcm", "ocm"]
    id: int
    initialClk: Optional[str] = None
    status: Optional[int] = None
    clk: str
    conflateMs: Optional[int] = None
    heartbeatMs: Optional[int] = None
    pt: int
    ct: str
    mc: List[MarketChange] = []

    @property
    def is_heartbeat(self):
        return self.ct == "HEARTBEAT"

    @property
    def stream_unreliable(self):
        return self.status == 503


class UnmatchedOrder(msgspec.Struct, frozen=True):  # type: ignore
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: str
    p: float
    s: float
    side: Literal["B", "L"]
    status: Literal["E", "EC"]
    pt: str
    ot: str
    pd: int
    md: Optional[int] = None
    cd: Optional[int] = None
    ld: Optional[int] = None
    avp: Optional[float] = None
    sm: Optional[float] = None
    sr: Optional[float] = None
    sl: Optional[float] = None
    sc: Optional[float] = None
    sv: Optional[float] = None
    rac: str
    rc: str
    rfo: str
    rfs: str


class OrderChanges(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    fullImage: Optional[bool] = False
    id: int
    uo: Optional[List[UnmatchedOrder]] = []
    mb: Optional[List[List]] = []
    ml: Optional[List[List]] = []


class OrderAccountChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: str
    fullImage: Optional[bool] = False
    orc: Optional[List[OrderChanges]] = []


class OrderChangeMessage(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    op: str
    id: int
    clk: str
    pt: int
    oc: List[OrderAccountChange] = []
