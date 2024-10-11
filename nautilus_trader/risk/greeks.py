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

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.nautilus_pyo3 import imply_vol_and_greeks
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.greeks import GreeksData
from nautilus_trader.model.greeks import InterestRateCurveData
from nautilus_trader.model.greeks import InterestRateData
from nautilus_trader.model.identifiers import InstrumentId


def greeks_key(instrument_id: InstrumentId):
    return f"{instrument_id}_GREEKS"


class GreeksCalculatorConfig(ActorConfig, frozen=True):
    """
    Configuration settings for the GreeksCalculator actor.

    Parameters
    ----------
    load_greeks : bool, default False
        Flag to determine whether to load pre-calculated Greeks.
    underlying : str, default "ES"
        The underlying asset symbol.
    bar_spec : str, default "1-MINUTE-LAST"
        The bar specification for data subscription.
    curve_name : str, default "USD_ShortTerm"
        The name of the interest rate curve.
    interest_rate : float, default 0.05
        The interest rate used for calculations.

    """

    load_greeks: bool = False
    underlying: str = "ES"
    bar_spec: str = "1-MINUTE-LAST"
    curve_name: str = "USD_ShortTerm"
    interest_rate: float = 0.05


class GreeksCalculator(Actor):
    """
    A class for calculating option Greeks for futures options.

    This calculator works specifically for European options on futures with no dividends.
    It computes the Greeks for all options of a given underlying when a bar of the future is received.

    Parameters
    ----------
    config : GreeksCalculatorConfig
        The configuration settings for the GreeksCalculator.

    Attributes
    ----------
    load_greeks : bool
        Flag to determine whether to load pre-calculated Greeks.
    underlying : str
        The underlying asset symbol.
    bar_spec : str
        The bar specification for data subscription.
    curve_name : str
        The name of the interest rate curve.
    interest_rate : float
        The interest rate used for calculations.

    Methods
    -------
    on_start()
        Initializes data subscriptions when the actor starts.
    on_data(data)
        Handles incoming data updates (GreeksData, InterestRateData or InterestRateCurveData).
    on_bar(bar: Bar)
        Processes incoming bar data and triggers Greek calculations.
    compute_greeks(instrument_id: InstrumentId, future_price: float, ts_event: int)
        Computes Greeks for options based on the future price.

    """

    def __init__(self, config: GreeksCalculatorConfig):
        super().__init__(config)

        self.load_greeks = config.load_greeks

        self.underlying = config.underlying
        self.bar_spec = config.bar_spec

        self.curve_name = config.curve_name
        self.interest_rate = InterestRateData(
            curve_name=self.curve_name,
            interest_rate=config.interest_rate,
        )

    def on_start(self):
        if self.load_greeks:
            self.subscribe_data(
                DataType(GreeksData, metadata={"instrument_id": f"{self.underlying}*"}),
            )
        else:
            self.msgbus.subscribe(
                topic=f"data.bars.{self.underlying}*-{self.bar_spec}*",
                handler=self.on_bar,
            )

        self.subscribe_data(DataType(InterestRateData, metadata={"curve_name": self.curve_name}))
        self.subscribe_data(
            DataType(InterestRateCurveData, metadata={"curve_name": self.curve_name}),
        )

    def on_data(self, data):
        if isinstance(data, GreeksData):
            self.cache_greeks(data)
        elif isinstance(data, InterestRateData) or isinstance(data, InterestRateCurveData):
            self.interest_rate = data

    def on_bar(self, bar: Bar):
        self.compute_greeks(bar.bar_type.instrument_id, float(bar.close), bar.ts_init)

    def compute_greeks(self, instrument_id: InstrumentId, future_price: float, ts_event: int):
        future_definition = self.cache.instrument(instrument_id)

        if future_definition.instrument_class is not InstrumentClass.FUTURE:
            return

        future_underlying = instrument_id.symbol.value
        multiplier = float(future_definition.multiplier)

        utc_now = unix_nanos_to_dt(ts_event)

        for option_definition in self.cache.instruments():
            if (
                option_definition.instrument_class is not InstrumentClass.OPTION
                or option_definition.underlying != future_underlying
            ):
                continue

            is_call = option_definition.option_kind is OptionKind.CALL
            strike = float(option_definition.strike_price)

            expiry_utc = option_definition.expiration_utc
            expiry = date_to_int(expiry_utc)
            expiry_in_years = min((expiry_utc - utc_now).days, 1) / 365.25

            interest_rate = self.interest_rate(expiry_in_years)
            self.log.debug(f"Interest rate for {option_definition.id}: {interest_rate}")

            option_mid_price = float(self.cache.price(option_definition.id, PriceType.MID))

            greeks = imply_vol_and_greeks(
                future_price,
                interest_rate,
                0.0,
                is_call,
                strike,
                expiry_in_years,
                option_mid_price,
                multiplier,
            )

            greeks_data = GreeksData(
                ts_event,
                ts_event,
                option_definition.id,
                is_call,
                strike,
                expiry,
                future_price,
                expiry_in_years,
                interest_rate,
                greeks.vol,
                greeks.price,
                greeks.delta,
                greeks.gamma,
                greeks.vega,
                greeks.theta,
                1.0,
                abs(greeks.delta / multiplier),
            )

            # write greeks to the cache
            self.cache_greeks(greeks_data)

            # publish greeks on message bus
            self.publish_data(
                DataType(GreeksData, metadata={"instrument_id": greeks_data.instrument_id.value}),
                greeks_data,
            )

    def cache_greeks(self, greeks_data: GreeksData):
        self.cache.add(greeks_key(greeks_data.instrument_id), greeks_data.to_bytes())


class InterestRateProviderConfig(ActorConfig, frozen=True):
    """
    Configuration for the InterestRateProvider actor.

    Parameters
    ----------
    interest_rates_file : str
        Path to the file containing interest rate data.
    curve_name : str, default "USD_ShortTerm"
        Name of the interest rate curve. Default is "USD_ShortTerm".

    """

    interest_rates_file: str
    curve_name: str = "USD_ShortTerm"


class InterestRateProvider(Actor):
    """
    A provider for interest rate data.

    This actor is responsible for importing interest rates from a file,
    updating the current interest rate, and publishing interest rate data
    on the message bus.

    Parameters
    ----------
    interest_rates_file : str
        Path to the file containing interest rate data.
    curve_name : str
        Name of the interest rate curve.

    Methods
    -------
    on_start()
        Initializes the interest rate data on actor start.
    update_interest_rate(alert=None)
        Updates and publishes the current interest rate.

    """

    def __init__(self, config: InterestRateProviderConfig):
        super().__init__(config)

        self.interest_rates_file = config.interest_rates_file
        self.curve_name = config.curve_name
        self.interest_rates_df = None

    def on_start(self):
        self.update_interest_rate()

    def update_interest_rate(self, alert=None):
        # import interest rates the first time
        if self.interest_rates_df is None:
            self.interest_rates_df = import_interest_rates(self.interest_rates_file)

        # get the interest rate for the current month
        utc_now_ns = alert.ts_init if alert is not None else self.clock.timestamp_ns()
        utc_now = unix_nanos_to_dt(utc_now_ns)
        month_string = f"{utc_now.year}-{str(utc_now.month).zfill(2)}"
        interest_rate_value = float(self.interest_rates_df.loc[month_string, "interest_rate"])

        interest_rate = InterestRateData(
            utc_now_ns,
            utc_now_ns,
            self.curve_name,
            interest_rate_value,
        )

        # publish interest rate on message bus
        self.publish_data(
            DataType(InterestRateData, metadata={"curve_name": interest_rate.curve_name}),
            interest_rate,
        )

        # set an alert to update for the next month
        next_month_start = next_month_start_from_timestamp(utc_now)
        self.clock.set_time_alert(
            "interest rate update",
            next_month_start,
            self.update_interest_rate,
            override=True,
        )


# example file usd_short_term_rate.xml in the current repo
# Can be downloaded from below link
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
