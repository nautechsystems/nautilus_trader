from typing import Union

from msgspec.json import Decoder

from nautilus_trader.adapters.betfair.parsing.spec.mcm import MCM
from nautilus_trader.adapters.betfair.parsing.spec.ocm import OCM
from nautilus_trader.adapters.betfair.parsing.spec.status import Connection
from nautilus_trader.adapters.betfair.parsing.spec.status import Status


STREAM_MESSAGE = Union[Connection, Status, MCM, OCM]
STREAM_DECODER = Decoder(STREAM_MESSAGE)
