from datetime import datetime

import pytz

from nautilus_trader.core.data import Data


UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


class MyData(Data):
    """
    Represents an example user defined data class.
    """

    def __init__(
        self,
        value,
        ts_event=0,
        ts_init=0,
    ):
        super().__init__(ts_event, ts_init)
        self.value = value
