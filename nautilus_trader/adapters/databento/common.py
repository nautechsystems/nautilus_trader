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

from pathlib import Path

import databento

from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


def check_file_path(path: Path) -> None:
    """
    Check that the file at the given `path` exists and is not empty.

    Parameters
    ----------
    path : Path
        The file path to check.

    Raises
    ------
    FileNotFoundError
        If a file is not found at the given `path`.
    ValueError
        If the file at the given `path` is empty.

    """
    if not path.is_file() or not path.exists():
        raise FileNotFoundError(path)

    if path.stat().st_size == 0:
        raise ValueError(
            f"Empty file found at {path}",
        )


def nautilus_instrument_id_from_databento(
    raw_symbol: str,
    publisher: DatabentoPublisher,
) -> InstrumentId:
    """
    Return the Nautilus `InstrumentId` parsed from the given `symbol` and `publisher`
    details.

    Parameters
    ----------
    raw_symbol : str
        The raw symbol for the identifier.
    publisher : DatebentoPublisher
        The Databento publisher details for the identifier.

    Returns
    -------
    InstrumentId

    Notes
    -----
    The Databento `instrument_id` is an integer, where as a Nautilus `InstrumentId` is a
    symbol and venue combination.

    """
    return InstrumentId(Symbol(raw_symbol), Venue(publisher.venue))


def databento_schema_from_nautilus_bar_type(bar_type: BarType) -> databento.Schema:
    """
    Return the Databento bar aggregate schema for the given Nautilus `bar_type`.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the schema.

    Returns
    -------
    databento.Schema

    Raises
    ------
    ValueError
        If any property of `bar_type` is invalid to map to a Databento schema.

    """
    PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

    if not bar_type.spec.is_time_aggregated():
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only time bars are aggregated by Databento).",
        )

    if bar_type.spec.price_type != PriceType.LAST:
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only `LAST` price bars are aggregated by Databento).",
        )

    if bar_type.spec.step != 1:
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only a step of 1 is supported by Databento).",
        )

    match bar_type.spec.aggregation:
        case BarAggregation.SECOND:
            return databento.Schema.OHLCV_1S
        case BarAggregation.MINUTE:
            return databento.Schema.OHLCV_1M
        case BarAggregation.HOUR:
            return databento.Schema.OHLCV_1H
        case BarAggregation.DAY:
            return databento.Schema.OHLCV_1D
        case _:
            raise ValueError(
                f"Invalid bar type '{bar_type}'. "
                "Use any of ['SECOND', 'MINTUE', 'HOUR', 'DAY'] time aggregations.",
            )
