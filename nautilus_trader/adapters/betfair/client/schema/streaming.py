from collections import namedtuple
from typing import List, Literal, Optional, Union

import msgspec

from nautilus_trader.adapters.betfair.common import EVENT_TYPE_TO_NAME


# class MarketSubscription(msgspec.Struct):
#     {"op": "marketSubscription", "id": 2,
#      "marketFilter": {"marketIds": ["1.120684740"], "bspMarket": true, "bettingTypes": ["ODDS"], "eventTypeIds": ["1"],
#                       "eventIds": ["27540841"], "turnInPlayEnabled": true, "marketTypes": ["MATCH_ODDS"],
#                       "countryCodes": ["ES"]}, "marketDataFilter": {}}


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

    sortPriority: int
    id: Union[int, str]
    name: Optional[str] = None
    hc: Optional[str] = None
    status: Optional[str] = None
    adjustmentFactor: Optional[float] = None


class MarketDefinition(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    bspMarket: bool
    turnInPlayEnabled: bool
    persistenceEnabled: bool
    marketBaseRate: float
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
    venue: Optional[str] = None
    countryCode: str
    discountAllowed: bool
    timezone: str
    openDate: str
    version: int

    @property
    def event_type_name(self) -> str:
        return EVENT_TYPE_TO_NAME[self.eventTypeId]

    def to_dict(self):
        return {f: getattr(self, f) for f in self.__struct_fields__}


class AvailableToBack(namedtuple("AvailableToBack", "price,volume")):
    """AvailableToBack"""

    pass


class AvailableToLay(namedtuple("AvailableToLay", "price,volume")):
    """AvailableToLay"""

    pass


class BestAvailableToBack(namedtuple("BestAvailableToBack", "level,price,volume")):
    """BestAvailableToBack"""

    pass


class BestAvailableToLay(namedtuple("BestAvailableToLay", "level,price,volume")):
    """BestAvailableToLay"""

    pass


class BestDisplayAvailableToBack(namedtuple("BestDisplayAvailableToBack", "level,price,volume")):
    """BestDisplayAvailableToBack"""

    pass


class BestDisplayAvailableToLay(namedtuple("BestDisplayAvailableToLay", "level,price,volume")):
    """BestDisplayAvailableToLay"""

    pass


class Trade(namedtuple("Trade", "price,volume")):
    """Trade"""

    pass


class StartingPriceBack(namedtuple("StartingPriceBack", "price,volume")):
    """StartingPriceBack"""

    pass


class StartingPriceLay(namedtuple("StartingPriceLay", "price,volume")):
    """StartingPriceLay"""

    pass


class RunnerChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    atb: Optional[List[AvailableToBack]] = []
    atl: Optional[List[AvailableToLay]] = []
    batb: Optional[List[BestAvailableToBack]] = []
    batl: Optional[List[BestAvailableToLay]] = []
    bdatb: Optional[List[BestDisplayAvailableToBack]] = []
    bdatl: Optional[List[BestDisplayAvailableToLay]] = []
    spb: Optional[List[StartingPriceBack]] = []
    spl: Optional[List[StartingPriceLay]] = []
    trd: Optional[List[Trade]] = []
    ltp: Optional[float] = None
    tv: Optional[float] = None
    id: Union[int, str]
    hc: Optional[float] = None


class MarketChange(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: str
    marketDefinition: Optional[MarketDefinition] = None
    rc: List[RunnerChange] = []
    img: bool = False
    tv: Optional[float] = None
    con: Optional[bool] = None


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


class OCM(msgspec.Struct, tag_field="op", tag=str.lower):  # type: ignore
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: int
    clk: str
    pt: int
    oc: List[OrderAccountChange] = []


class Connection(msgspec.Struct, tag_field="op", tag=str.lower):  # type: ignore
    """
    Connection Message
    """

    connectionId: str


class Status(msgspec.Struct, tag_field="op", tag=str.lower):  # type: ignore
    """
    Status Message
    """

    statusCode: Literal["SUCCESS", "FAILURE"]
    connectionClosed: bool
    errorCode: Optional[str] = None
    errorMessage: Optional[str] = None
    connectionsAvailable: Optional[int] = None


class MCM(msgspec.Struct, tag_field="op", tag=str.lower):  # type: ignore
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: Optional[int] = None
    initialClk: Optional[str] = None
    status: Optional[int] = None
    clk: str
    conflateMs: Optional[int] = None
    heartbeatMs: Optional[int] = None
    pt: int
    ct: Optional[Literal["HEARTBEAT", "SUB_IMAGE", "RESUB_DELTA"]] = None
    mc: List[MarketChange] = []

    @property
    def is_heartbeat(self):
        return self.ct == "HEARTBEAT"

    @property
    def stream_unreliable(self):
        return self.status == 503


def stream_decode(raw: bytes) -> Union[Connection, Status, MCM, OCM]:
    return msgspec.json.decode(raw, type=Union[Status, Connection, MCM, OCM])
