from datetime import date
from decimal import Decimal

import pandas as pd

from stubs.model.identifiers import InstrumentId

class RolloverInterestCalculator:

    def __init__(self, data: pd.DataFrame): ...
    def get_rate_data(self) -> dict[str, pd.DataFrame]: ...
    def calc_overnight_rate(self, instrument_id: InstrumentId, date: date) -> Decimal: ...
