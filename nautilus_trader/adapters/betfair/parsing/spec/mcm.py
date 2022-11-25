from collections import namedtuple
from typing import Literal, Optional, Union

import msgspec

from nautilus_trader.adapters.betfair.common import EVENT_TYPE_TO_NAME


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
    selectionId: Optional[str] = None


class MarketDefinition(msgspec.Struct):
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    bspMarket: bool
    turnInPlayEnabled: bool
    persistenceEnabled: bool
    marketBaseRate: float
    marketId: Optional[str] = ""
    marketName: Optional[str] = ""
    marketStartTime: Optional[str] = ""
    eventId: str
    eventTypeId: str
    numberOfWinners: int
    bettingType: str
    marketType: str
    marketTime: str
    competitionId: Optional[str] = ""
    competitionName: Optional[str] = ""
    eventName: Optional[str] = ""
    suspendTime: str
    bspReconciled: bool
    complete: bool
    inPlay: bool
    crossMatching: bool
    runnersVoidable: bool
    numberOfActiveRunners: int
    betDelay: int
    status: str
    runners: list[Runner]
    regulators: list[str]
    venue: Optional[str] = None
    countryCode: Optional[str] = None
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

    atb: Optional[list[AvailableToBack]] = []
    atl: Optional[list[AvailableToLay]] = []
    batb: Optional[list[BestAvailableToBack]] = []
    batl: Optional[list[BestAvailableToLay]] = []
    bdatb: Optional[list[BestDisplayAvailableToBack]] = []
    bdatl: Optional[list[BestDisplayAvailableToLay]] = []
    spb: Optional[list[StartingPriceBack]] = []
    spl: Optional[list[StartingPriceLay]] = []
    trd: Optional[list[Trade]] = []
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
    rc: list[RunnerChange] = []
    img: bool = False
    tv: Optional[float] = None
    con: Optional[bool] = None


class MCM(msgspec.Struct, tag_field="op", tag=str.lower):  # type: ignore
    """
    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/Exchange+Stream+API
    """

    id: Optional[int] = None
    initialClk: Optional[str] = None
    status: Optional[int] = None
    clk: Optional[str]
    conflateMs: Optional[int] = None
    heartbeatMs: Optional[int] = None
    pt: int
    ct: Optional[Literal["HEARTBEAT", "SUB_IMAGE", "RESUB_DELTA"]] = None
    mc: list[MarketChange] = []

    @property
    def is_heartbeat(self):
        return self.ct == "HEARTBEAT"

    @property
    def stream_unreliable(self):
        return self.status == 503
