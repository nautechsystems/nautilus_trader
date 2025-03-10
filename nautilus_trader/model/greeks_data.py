# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from dataclasses import field

import numpy as np

from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.core.math import quadratic_interpolation
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.identifiers import InstrumentId


@customdataclass
class GreeksData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    is_call: bool = True
    strike: float = 0.0
    expiry: int = 0
    expiry_in_years: float = 0.0
    multiplier: float = 0.0
    quantity: float = 0.0

    underlying_price: float = 0.0
    interest_rate: float = 0.0
    cost_of_carry: float = 0.0

    vol: float = 0.0
    pnl: float = 0.0
    price: float = 0.0
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    # in the money probability, P(phi * S_T > phi * K), phi = 1 if is_call else -1
    itm_prob: float = 0.0

    def __repr__(self):
        return (
            f"GreeksData(instrument_id={self.instrument_id}, "
            f"expiry={self.expiry}, itm_prob={self.itm_prob * 100:.2f}%, "
            f"vol={self.vol * 100:.2f}%, pnl={self.pnl:,.2f}, , price={self.price:,.2f}, delta={self.delta:,.2f}, "
            f"gamma={self.gamma:,.2f}, vega={self.vega:,.2f}, theta={self.theta:,.2f}, "
            f"quantity={self.quantity}, ts_init={unix_nanos_to_iso8601(self.ts_init)})"
        )

    @classmethod
    def from_delta(
        cls,
        instrument_id: InstrumentId,
        delta: float,
        multiplier: float,
        ts_event: int = 0,
    ):
        return GreeksData(
            ts_event,
            ts_event,
            instrument_id=instrument_id,
            multiplier=multiplier,
            delta=delta,
            quantity=1.0,
        )

    def __rmul__(self, quantity):  # quantity * greeks
        return GreeksData(
            self.ts_init,
            self.ts_event,
            self.instrument_id,
            self.is_call,
            self.strike,
            self.expiry,
            self.expiry_in_years,
            self.multiplier,
            self.quantity,
            self.underlying_price,
            self.interest_rate,
            self.cost_of_carry,
            self.vol,
            quantity * self.pnl,
            quantity * self.price,
            quantity * self.delta,
            quantity * self.gamma,
            quantity * self.vega,
            quantity * self.theta,
            self.itm_prob,
        )


@customdataclass
class PortfolioGreeks(Data):
    pnl: float = 0.0
    price: float = 0.0
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    def __repr__(self):
        return (
            f"PortfolioGreeks(pnl={self.pnl:,.2f}, price={self.price:,.2f}, delta={self.delta:,.2f}, gamma={self.gamma:,.2f}, "
            f"vega={self.vega:,.2f}, theta={self.theta:,.2f}, "
            f"ts_event={unix_nanos_to_iso8601(self.ts_event)}, ts_init={unix_nanos_to_iso8601(self.ts_init)})"
        )

    def __add__(self, other):
        return PortfolioGreeks(
            self.ts_event,
            self.ts_init,
            self.pnl + other.pnl,
            self.price + other.price,
            self.delta + other.delta,
            self.gamma + other.gamma,
            self.vega + other.vega,
            self.theta + other.theta,
        )


@customdataclass
class YieldCurveData(Data):
    """
    Represents a yield curve with associated tenors and rates.

    This class stores information about an interest rate curve (zero-rates, used for example for discount factors of the form
    exp(- r * t) for example), including its name, tenors (time points), and corresponding rates.
    It provides methods for interpolation and data conversion.

    Attributes:
        curve_name (str): The name of the yield curve.
        tenors (np.ndarray): An array of tenor points (in years).
        interest_rates (np.ndarray): An array of interest rates corresponding to the tenors.

    Methods:
        __call__: Interpolates the yield curve for a given expiry time.

    """

    curve_name: str = "USD"
    tenors: np.ndarray = field(default_factory=lambda: np.array([0.5, 1.0, 1.5, 2.0, 2.5]))
    interest_rates: np.ndarray = field(
        default_factory=lambda: np.array([0.04, 0.04, 0.04, 0.04, 0.04]),
    )

    def __repr__(self):
        return (
            f"InterestRateCurve(curve_name={self.curve_name}, "
            f"ts_event={unix_nanos_to_iso8601(self.ts_event)}, ts_init={unix_nanos_to_iso8601(self.ts_init)})"
        )

    def __call__(self, expiry_in_years: float) -> float:
        if len(self.interest_rates) == 1:
            return self.interest_rates[0]

        return quadratic_interpolation(expiry_in_years, self.tenors, self.interest_rates)

    def to_dict(self, to_arrow=False):
        result = {
            "curve_name": self.curve_name,
            "tenors": self.tenors.tobytes(),
            "interest_rates": self.interest_rates.tobytes(),
            "type": "YieldCurveData",
            "ts_event": self._ts_event,
            "ts_init": self._ts_init,
        }

        if to_arrow:
            result["date"] = int(unix_nanos_to_dt(result["ts_event"]).strftime("%Y%m%d"))

        return result

    @classmethod
    def from_dict(cls, data):
        data.pop("type", None)
        data.pop("date", None)

        data["tenors"] = np.frombuffer(data["tenors"])
        data["interest_rates"] = np.frombuffer(data["interest_rates"])

        return YieldCurveData(**data)
