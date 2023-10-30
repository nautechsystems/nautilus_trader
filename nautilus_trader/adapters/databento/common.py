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

from nautilus_trader.adapters.databento.types import DatabentoPublisher
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

    Notes
    -----
    The Databento `instrument_id` is an integer, where as a Nautilus `InstrumentId` is a
    symbol and venue combination.

    Parameters
    ----------
    raw_symbol : str
        The raw symbol for the identifier.
    publisher : DatebentoPublisher
        The Databento publisher details for the identifier.

    Returns
    -------
    InstrumentId

    """
    return InstrumentId(Symbol(raw_symbol), Venue(publisher.venue))
