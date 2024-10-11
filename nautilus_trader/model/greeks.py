# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.math import quadratic_interpolation
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_str
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.identifiers import InstrumentId


@customdataclass
class GreeksData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    is_call: bool = True
    strike: float = 0.0
    expiry: int = 0

    underlying_price: float = 0.0
    expiry_in_years: float = 0.0
    interest_rate: float = 0.0

    vol: float = 0.0
    price: float = 0.0
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    quantity: float = 0.0
    # in the money probability, P(phi * S_T > phi * K), phi = 1 if is_call else -1
    itm_prob: float = 0.0

    def __repr__(self):
        return (
            f"GreeksData(instrument_id={self.instrument_id}, "
            f"expiry={self.expiry}, itm_prob={self.itm_prob * 100:.2f}%, "
            f"vol={self.vol * 100:.2f}%, price={self.price:.2f}, delta={self.delta:.2f}, "
            f"gamma={self.gamma:.2f}, vega={self.vega:.2f}, theta={self.theta:.2f}, quantity={self.quantity}, "
            f"ts_event={unix_nanos_to_str(self.ts_event)}, ts_init={unix_nanos_to_str(self.ts_init)})"
        )

    @classmethod
    def from_delta(cls, instrument_id: InstrumentId, delta: float):
        return GreeksData(instrument_id=instrument_id, delta=delta, quantity=1.0)

    def __rmul__(self, quantity):  # quantity * greeks
        return GreeksData(
            self.ts_init,
            self.ts_event,
            self.instrument_id,
            self.is_call,
            self.strike,
            self.expiry,
            self.underlying_price,
            self.expiry_in_years,
            self.interest_rate,
            self.vol,
            quantity * self.price,
            quantity * self.delta,
            quantity * self.gamma,
            quantity * self.vega,
            quantity * self.theta,
            quantity * self.quantity,
            self.itm_prob,
        )


@customdataclass
class PortfolioGreeks(Data):
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    def __repr__(self):
        return (
            f"PortfolioGreeks(delta={self.delta:.2f}, gamma={self.gamma:.2f}, vega={self.vega:.2f}, theta={self.theta:.2f}, "
            f"ts_event={unix_nanos_to_str(self.ts_event)}, ts_init={unix_nanos_to_str(self.ts_init)})"
        )

    def __add__(self, other):
        return PortfolioGreeks(
            self.ts_event,
            self.ts_init,
            self.delta + other.delta,
            self.gamma + other.gamma,
            self.vega + other.vega,
            self.theta + other.theta,
        )


@customdataclass
class InterestRateData(Data):
    """
    Represents interest rate data for a specific curve.

    This class stores information about an interest rate, including the curve name
    and the interest rate value. It provides methods for string representation and
    callable functionality to return the interest rate.

    Attributes:
        curve_name (str): The name of the interest rate curve. Defaults to "USD_ShortTerm".
        interest_rate (float): The interest rate value. Defaults to 0.05 (5%).

    """

    curve_name: str = "USD_ShortTerm"
    interest_rate: float = 0.05

    def __repr__(self):
        return (
            f"InterestRateData(curve_name={self.curve_name}, interest_rate={self.interest_rate * 100:.2f}%, "
            f"ts_event={unix_nanos_to_str(self.ts_event)}, ts_init={unix_nanos_to_str(self.ts_init)})"
        )

    def __call__(self, expiry_in_years: float):
        return self.interest_rate


@customdataclass
class InterestRateCurveData(Data):
    """
    Represents an interest rate curve with associated tenors and rates.

    This class stores information about an interest rate curve (zero-rates, used for discount factors of the form
    exp(- r * t) for example), including its name, tenors (time points), and corresponding interest rates.
    It provides methods for interpolation and data conversion.

    Attributes:
        curve_name (str): The name of the interest rate curve.
        tenors (np.ndarray): An array of tenor points (in years).
        interest_rates (np.ndarray): An array of interest rates corresponding to the tenors.

    Methods:
        __call__: Interpolates the interest rate for a given expiry time.

    """

    curve_name: str = "USD_ShortTerm"
    tenors: np.ndarray = field(default_factory=lambda: np.array([0.5, 1.0, 1.5, 2.0, 2.5]))
    interest_rates: np.ndarray = field(
        default_factory=lambda: np.array([0.04, 0.04, 0.04, 0.04, 0.04]),
    )

    def __repr__(self):
        return (
            f"InterestRateCurve(curve_name={self.curve_name}, "
            f"ts_event={unix_nanos_to_str(self.ts_event)}, ts_init={unix_nanos_to_str(self.ts_init)})"
        )

    def __call__(self, expiry_in_years: float):
        return quadratic_interpolation(expiry_in_years, self.tenors, self.interest_rates)

    def to_dict(self, to_arrow=False):
        result = {
            "curve_name": self.curve_name,
            "tenors": self.tenors.tobytes(),
            "interest_rates": self.interest_rates.tobytes(),
            "type": "InterestRateCurveData",
            "ts_event": self._ts_event,
            "ts_init": self._ts_init,
        }

        if to_arrow:
            result["date"] = int(unix_nanos_to_dt(result["ts_event"]).strftime("%Y%m%d"))

        return result

    def from_dict(self, data):
        data.pop("type", None)
        data.pop("date", None)

        data["tenors"] = np.frombuffer(data["tenors"])
        data["interst_rates"] = np.frombuffer(data["interest_rates"])

        return InterestRateCurveData(**data)
