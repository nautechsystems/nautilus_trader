from typing import List, Literal, Optional

import msgspec


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
