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

import pandas as pd

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.greeks import GreeksData

from nautilus_trader.common.actor cimport Actor
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport greeks_black_scholes_greeks
from nautilus_trader.core.rust.model cimport greeks_imply_vol
from nautilus_trader.core.rust.model cimport greeks_imply_vol_and_greeks
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport InstrumentId


def greeks_key(instrument_id: InstrumentId):
    return f"{instrument_id}_GREEKS"


cpdef dict black_scholes_greeks(double s, double r, double b, double vol, bint is_call, double k, double t,
                                double multiplier):
    greeks = greeks_black_scholes_greeks(s, r, b, vol, is_call, k, t, multiplier)
    return dict(vol=vol, price=greeks.price, delta=greeks.delta, gamma=greeks.gamma, vega=greeks.vega,
                theta=greeks.theta)


cpdef double imply_vol(double s, double r, double b, bint is_call, double k, double t, double price):
    return greeks_imply_vol(s, r, b, is_call, k, t, price)


cpdef dict imply_vol_and_greeks(double s, double r, double b, bint is_call, double k, double t, double price,
                                      double multiplier):
    greeks = greeks_imply_vol_and_greeks(s, r, b, is_call, k, t, price, multiplier)
    return dict(vol=greeks.vol, price=greeks.price, delta=greeks.delta, gamma=greeks.gamma, vega=greeks.vega,
                theta=greeks.theta)

# temporary before tests
# s = 100.0
# k = 100.1
# t = 1.0
# r = 0.01
# b = 0.005
# sigma = 0.2
# is_call = True

# greeks = black_scholes_greeks(s, r, b, sigma, is_call, k, t, 1.)
# imply_vol(s, r, b, is_call, k, t, greeks['price'])
# imply_vol_and_greeks(s, r, b, is_call, k, t, greeks['price'], 1.)


class GreeksCalculatorConfig(ActorConfig, frozen=True):
    load_greeks: bool = False
    underlying: str = "ES"
    update_period: str = "1-MINUTE"
    interest_rates_file: str | None = None
    interest_rate: float = 0.05


cdef class GreeksCalculator(Actor):
    def __init__(self, config: GreeksCalculatorConfig):
        super().__init__(config)

        self.load_greeks = config.load_greeks
        self.underlying = config.underlying
        self.update_period = config.update_period
        self.interest_rates_file = config.interest_rates_file
        self.interest_rate = config.interest_rate
        self.interest_rates_df = None

    def set_interest_rate(self, alert=None):
        if self.interest_rates_file is None:
            return

        # importing interest rates the first time
        if self.interest_rates_df is None:
            self.interest_rates_df = import_interest_rates(self.interest_rates_file)

        # setting the interest rate for the current month
        utc_now = self.clock.utc_now()
        month_string = f"{utc_now.year}-{str(utc_now.month).zfill(2)}"
        self.interest_rate = float(self.interest_rates_df.loc[month_string, "interest_rate"])

        # setting an alert for the next month
        next_month_start = next_month_start_from_timestamp(utc_now)
        self.clock.set_time_alert("interest rate update", next_month_start, self.set_interest_rate, override=True)

    cpdef void on_start(self):
        self.set_interest_rate()

        if self.load_greeks:
            self.subscribe_to_greeks()
        else:
            self.msgbus.subscribe(topic=f"data.bars.{self.underlying}*", handler=self.on_bar)

    cpdef void on_data(self, data):
        self.cache_greeks(data)

    cpdef void on_bar(self, bar: Bar):
        if self.update_period in str(bar.bar_type):
            self.compute_greeks(bar.bar_type.instrument_id, float(bar.close), bar.ts_init)

    def compute_greeks(self, instrument_id: InstrumentId, future_price: float, ts_event: int):
        future_definition = self.cache.instrument(instrument_id)

        if future_definition.instrument_class is not InstrumentClass.FUTURE:
            return

        r = self.interest_rate

        future_underlying = instrument_id.symbol.value
        multiplier = float(future_definition.multiplier)

        utc_now = unix_nanos_to_dt(ts_event)  # self.clock.utc_now()

        for option_definition in self.cache.instruments():
            if (option_definition.instrument_class is not InstrumentClass.OPTION
                or option_definition.underlying != future_underlying):
                continue

            is_call = option_definition.option_kind is OptionKind.CALL
            strike = float(option_definition.strike_price)
            expiry_utc = option_definition.expiration_utc
            expiry = date_to_int(expiry_utc)
            expiry_in_years = min((expiry_utc - utc_now).days, 1) / 365.0

            option_mid_price = float(self.cache.price(option_definition.id, PriceType.MID))

            greeks = greeks_imply_vol_and_greeks(
                future_price,
                r,
                0.0,
                is_call,
                strike,
                expiry_in_years,
                option_mid_price,
                multiplier,
            )
            # ts_init = self.clock.timestamp_ns()
            greeks_data = GreeksData(
                ts_event,
                ts_event,
                option_definition.id,
                is_call,
                strike,
                expiry,
                future_price,
                expiry_in_years,
                r,
                greeks.vol,
                greeks.price,
                greeks.delta,
                greeks.gamma,
                greeks.vega,
                greeks.theta,
            )

            self.cache_greeks(greeks_data)
            self.publish_greeks(greeks_data)

    def cache_greeks(self, greeks_data: GreeksData):
        self.cache.add(greeks_key(greeks_data.instrument_id), greeks_data.to_bytes())

    def publish_greeks(self, greeks_data: GreeksData):
        self.publish_data(
            DataType(GreeksData, metadata={"instrument_id": greeks_data.instrument_id.value}),
            greeks_data,
        )

    def subscribe_to_greeks(self):
        self.subscribe_data(DataType(GreeksData, metadata={"instrument_id": f"{self.underlying}*"}))


# download this file
# https://sdmx.oecd.org/public/rest/data/OECD.SDD.STES,DSD_STES@DF_FINMARK,4.0/USA.M.IR3TIB.PA.....?startPeriod=2020-01
# https://data-explorer.oecd.org/vis?lc=en&pg=0&fs[0]=Topic%2C1%7CEconomy%23ECO%23%7CShort-term%20economic%20statistics%23ECO_STS%23&fc=Frequency%20of%20observation&bp=true&snb=54&vw=tb&df[ds]=dsDisseminateFinalDMZ&df[id]=DSD_STES%40DF_FINMARK&df[ag]=OECD.SDD.STES&df[vs]=4.0&dq=USA.M.IR3TIB.PA.....&lom=LASTNPERIODS&lo=5&to[TIME_PERIOD]=false&ly[cl]=TIME_PERIOD
def import_interest_rates(xml_interest_rate_file):
    import xmltodict

    data_dict = None

    with open(xml_interest_rate_file) as xml_file:
        data_dict = xmltodict.parse(xml_file.read())

    interest_rates = [
        (x["generic:ObsDimension"]["@value"], float(x["generic:ObsValue"]["@value"]))
        for x in data_dict["message:GenericData"]["message:DataSet"]["generic:Series"][
            "generic:Obs"
        ]
    ]
    interest_rates.sort(key=lambda x: x[0])

    return (
            pd.DataFrame(interest_rates, columns=["month", "interest_rate"]).set_index("month") / 100.0
    )

def next_month_start_from_timestamp(timestamp):
    return (timestamp + pd.offsets.MonthBegin(1)).replace(
        hour=0,
        minute=0,
        second=0,
        microsecond=0,
        nanosecond=0,
    )

def date_to_string(date, string_format="%Y%m%d"):
    return date.strftime(string_format)

def date_to_int(date):
    return int(date_to_string(date))
