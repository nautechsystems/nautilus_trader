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

from os import PathLike
from typing import Any

import numpy as np
import pandas as pd

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import DataType
from nautilus_trader.model.greeks_data import YieldCurveData


class CSVTickDataLoader:
    """
    Provides a generic tick data CSV file loader.
    """

    @staticmethod
    def load(
        file_path: PathLike[str] | str,
        index_col: str | int = "timestamp",
        parse_dates: bool = True,
        datetime_format: str = "mixed",
        **kwargs: Any,
    ) -> pd.DataFrame:
        """
        Return a tick `pandas.DataFrame` loaded from the given CSV `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.
        index_col : str or int, default 'timestamp'
            The column to use as the row labels of the DataFrame.
        parse_dates : bool, default True
            If True, attempt to parse the index.
        datetime_format : str, default 'mixed'
            The timestamp column format.
        **kwargs : Any
            The additional parameters to be passed to pd.read_csv.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(
            file_path,
            index_col=index_col,
            parse_dates=parse_dates,
            **kwargs,
        )
        df.index = pd.to_datetime(df.index, format=datetime_format)
        return df


class CSVBarDataLoader:
    """
    Provides a generic bar data CSV file loader.
    """

    @staticmethod
    def load(
        file_path: PathLike[str] | str,
        index_col: str | int = "timestamp",
        parse_dates: bool = True,
        **kwargs: Any,
    ) -> pd.DataFrame:
        """
        Return the bar `pandas.DataFrame` loaded from the given CSV `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the CSV file.
        index_col : str | int, default 'timestamp'
            The column to use as the row labels of the DataFrame.
        parse_dates : bool, default True
            If True, attempt to parse the index.
        **kwargs : Any
            The additional parameters to be passed to pd.read_csv.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_csv(
            file_path,
            index_col=index_col,
            parse_dates=parse_dates,
            **kwargs,
        )
        df.index = pd.to_datetime(df.index, format="mixed")
        return df


class ParquetTickDataLoader:
    """
    Provides a generic tick data Parquet file loader.
    """

    @staticmethod
    def load(
        file_path: PathLike[str] | str,
        timestamp_column: str = "timestamp",
    ) -> pd.DataFrame:
        """
        Return the tick `pandas.DataFrame` loaded from the given Parquet `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the Parquet file.
        timestamp_column: str
            Name of the timestamp column in the parquet data

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_parquet(file_path)
        df = df.set_index(timestamp_column)
        return df


class ParquetBarDataLoader:
    """
    Provides a generic bar data Parquet file loader.
    """

    @staticmethod
    def load(file_path: PathLike[str] | str) -> pd.DataFrame:
        """
        Return the bar `pandas.DataFrame` loaded from the given Parquet `file_path`.

        Parameters
        ----------
        file_path : str, path object or file-like object
            The path to the Parquet file.

        Returns
        -------
        pd.DataFrame

        """
        df = pd.read_parquet(file_path)
        df = df.set_index("timestamp")
        return df


class InterestRateProviderConfig(ActorConfig, frozen=True):
    """
    Configuration for the InterestRateProvider actor.

    Parameters
    ----------
    interest_rates_file : str
        Path to the file containing interest rate data.
    curve_name : str, default "USD"
        Name of the interest rate curve.

    """

    interest_rates_file: str
    curve_name: str = "USD"


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
        self.interest_rates_df = None

    def on_start(self):
        self.update_interest_rate()

    def update_interest_rate(self, alert=None):
        # import interest rates the first time
        if self.interest_rates_df is None:
            self.interest_rates_df = import_interest_rates(self.config.interest_rates_file)

        # get the interest rate for the current month
        utc_now_ns = alert.ts_init if alert is not None else self.clock.timestamp_ns()
        utc_now = unix_nanos_to_dt(utc_now_ns)
        month_string = f"{utc_now.year}-{str(utc_now.month).zfill(2)}"  # 2024-01
        interest_rate_value = float(self.interest_rates_df.loc[month_string, "interest_rate"])

        yield_curve = YieldCurveData(
            utc_now_ns,
            utc_now_ns,
            self.config.curve_name,
            np.array([0.0]),
            np.array([interest_rate_value]),
        )

        # caching interest rate data
        self.cache.add_yield_curve(yield_curve)

        # publish interest rate on message bus
        self.publish_data(
            DataType(YieldCurveData, metadata={"curve_name": yield_curve.curve_name}),
            yield_curve,
        )

        # set an alert to update for the next month
        next_month_start = next_month_start_from_timestamp(utc_now)
        self.clock.set_time_alert(
            "interest rate update",
            next_month_start,
            self.update_interest_rate,
            override=True,
        )

    def on_stop(self):
        self.clock.cancel_timers()


# example file usd_short_term_rate.xml in the current repo
# Can be downloaded from below link
# https://sdmx.oecd.org/public/rest/data/OECD.SDD.STES,DSD_STES@DF_FINMARK,4.0/USA.M.IR3TIB.PA.....?startPeriod=2020-01
# https://data-explorer.oecd.org/vis?lc=en&pg=0&fs[0]=Topic%2C1%7CEconomy%23ECO%23%7CShort-term%20economic%20statistics%23ECO_STS%23&fc=Frequency%20of%20observation&bp=true&snb=54&vw=tb&df[ds]=dsDisseminateFinalDMZ&df[id]=DSD_STES%40DF_FINMARK&df[ag]=OECD.SDD.STES&df[vs]=4.0&dq=USA.M.IR3TIB.PA.....&lom=LASTNPERIODS&lo=5&to[TIME_PERIOD]=false&ly[cl]=TIME_PERIOD
def import_interest_rates(xml_interest_rate_file):
    import xmltodict

    with open(xml_interest_rate_file) as xml_file:
        data_dict = xmltodict.parse(xml_file.read())

    interest_rates = [
        (x["generic:ObsDimension"]["@value"], float(x["generic:ObsValue"]["@value"]))
        for x in data_dict["message:GenericData"]["message:DataSet"]["generic:Series"][
            "generic:Obs"
        ]
    ]

    return (
        pd.DataFrame(interest_rates, columns=["month", "interest_rate"])
        .set_index("month")
        .sort_index()
        / 100.0
    )


def next_month_start_from_timestamp(timestamp):
    return (timestamp + pd.offsets.MonthBegin(1)).floor(freq="d")
