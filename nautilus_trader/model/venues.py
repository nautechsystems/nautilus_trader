from typing import Final

from nautilus_trader.model.identifiers import Venue


# CME Globex exchanges
CBCM: Final[Venue] = Venue.from_code("CBCM")
GLBX: Final[Venue] = Venue.from_code("GLBX")
NYUM: Final[Venue] = Venue.from_code("NYUM")
XCBT: Final[Venue] = Venue.from_code("XCBT")
XCEC: Final[Venue] = Venue.from_code("XCEC")
XCME: Final[Venue] = Venue.from_code("XCME")
XFXS: Final[Venue] = Venue.from_code("XFXS")
XNYM: Final[Venue] = Venue.from_code("XNYM")
