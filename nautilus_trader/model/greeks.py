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

from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_str
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.identifiers import InstrumentId


@customdataclass
class GreeksData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    is_call: bool = True
    strike: float = 0.0
    expiry: int = 0

    forward: float = 0.0
    expiry_in_years: float = 0.0
    interest_rate: float = 0.0

    vol: float = 0.0
    price: float = 0.0
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    quantity: int = 1

    def __repr__(self):
        return (
            f"GreeksData(ts_init={unix_nanos_to_str(self.ts_init)}, instrument_id={self.instrument_id}, "
            f"expiry={self.expiry}, vol%={self.vol * 100:.2f}, price={self.price:.2f}, delta={self.delta:.2f}, "
            f"gamma={self.gamma:.2f}, vega={self.vega:.2f}, theta={self.theta:.2f}, quantity={self.quantity})"
        )

    @classmethod
    def from_delta(cls, instrument_id: InstrumentId, delta: float):
        return GreeksData(instrument_id=instrument_id, delta=delta)

    def __rmul__(self, quantity):  # quantity * greeks
        return GreeksData(
            self.ts_init,
            self.ts_event,
            self.instrument_id,
            self.is_call,
            self.strike,
            self.expiry,
            self.forward,
            self.expiry_in_years,
            self.interest_rate,
            self.vol,
            quantity * self.price,
            quantity * self.delta,
            quantity * self.gamma,
            quantity * self.vega,
            quantity * self.theta,
            quantity * self.quantity,
        )


@customdataclass
class PortfolioGreeks(Data):
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    def __repr__(self):
        return (
            f"PortfolioGreeks(ts_init={unix_nanos_to_str(self.ts_init)}, delta={self.delta:.2f}, "
            f"gamma={self.gamma:.2f}, vega={self.vega:.2f}, theta={self.theta:.2f})"
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
