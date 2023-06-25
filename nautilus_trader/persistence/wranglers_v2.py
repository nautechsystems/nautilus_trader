# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import polars as pl

# fmt: off
from nautilus_trader.core.nautilus_pyo3.model import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3.model import TradeTick as RustTradeTick
from nautilus_trader.core.nautilus_pyo3.persistence import QuoteTickDataWrangler as RustQuoteTickDataWrangler
from nautilus_trader.core.nautilus_pyo3.persistence import TradeTickDataWrangler as RustTradeTickDataWrangler
from nautilus_trader.model.instruments import Instrument


# fmt: on


class QuoteTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `QuoteTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `QuoteTick` and
    will not work the same way of the current wranglers which build the main `Cython` quote ticks.

    """

    def __init__(self, instrument: Instrument) -> None:
        self.instrument = instrument
        self._inner_wrangler = RustQuoteTickDataWrangler(
            instrument_id=instrument.id.value,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
        )

    def process(
        self,
        data: pl.DataFrame,
        default_size: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[RustQuoteTick]:
        """
        Process the given quote tick data into Nautilus `QuoteTick` objects.

        Expects columns ['bid', 'ask'] with 'timestamp' index.
        Note: The 'bid_size' and 'ask_size' columns are optional, will then use
        the `default_size`.

        Parameters
        ----------
        data : polars.DataFrame
            The quote tick data frame to process.
        default_size : float, default 1_000_000.0
            The default size for the bid and ask size of each tick (if not provided).
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustQuoteTick]
            A list of PyO3 [pyclass] `QuoteTick` objects.

        """
        return self._inner_wrangler.process(
            data=data,
            default_size=default_size,
            ts_init_delta=ts_init_delta,
        )

    def process_from_pandas(
        self,
        data: pd.DataFrame,
        default_size: float = 1_000_000.0,
        ts_init_delta: int = 0,
    ) -> list[RustQuoteTick]:
        """
        Process the given quote tick data into Nautilus `QuoteTick` objects.

        Expects columns ['bid', 'ask'] with 'timestamp' index.
        Note: The 'bid_size' and 'ask_size' columns are optional, will then use
        the `default_size`.

        Parameters
        ----------
        data : pandas.DataFrame
            The quote tick data frame to process.
        default_size : float, default 1_000_000.0
            The default size for the bid and ask size of each tick (if not provided).
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustQuoteTick]
            A list of PyO3 [pyclass] `QuoteTick` objects.

        """
        return self.process(
            data=pl.from_pandas(data),
            default_size=default_size,
            ts_init_delta=ts_init_delta,
        )


class TradeTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `TradeTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    Warnings
    --------
    This wrangler is used to build the PyO3 exposed version of `TradeTick` and
    will not work the same way of the current wranglers which build the main `Cython` trade ticks.

    """

    def __init__(self, instrument: Instrument) -> None:
        self.instrument = instrument
        self._inner_wrangler = RustTradeTickDataWrangler(
            instrument_id=instrument.id.value,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
        )

    def process(
        self,
        data: pl.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[RustTradeTick]:
        """
        Process the given trade tick data into Nautilus `TradeTick` objects.

        Parameters
        ----------
        data : polars.DataFrame
            The trade tick data frame to process.
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustTradeTick]
            A list of PyO3 [pyclass] `TradeTick` objects.

        """
        return self._inner_wrangler.process(
            data=data,
            ts_init_delta=ts_init_delta,
        )

    def process_from_pandas(
        self,
        data: pd.DataFrame,
        ts_init_delta: int = 0,
    ) -> list[RustTradeTick]:
        """
        Process the given trade tick data into Nautilus `TradeTick` objects.

        Parameters
        ----------
        data : pandas.DataFrame
            The trade tick data frame to process.
        ts_init_delta : int, default 0
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[RustTradeTick]
            A list of PyO3 [pyclass] `TradeTick` objects.

        """
        # Pre-processing
        if "side" in data.columns:
            data["side"] = data["side"].apply(lambda x: self._normalize_aggressor_side(x))
        data["trade_id"] = data["trade_id"].astype(str)

        # Convert Pandas to Polars
        df_pl = pl.from_pandas(data)
        index_column = pl.Series("timestamp", data.index)
        df_pl = pl.DataFrame({"timestamp": index_column, **data})

        return self.process(
            data=df_pl,
            ts_init_delta=ts_init_delta,
        )

    def _normalize_aggressor_side(self, value) -> str:
        return "BUYER" if str(value).upper() == "BUY" else "SELLER"
