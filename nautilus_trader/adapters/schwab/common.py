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
from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from decimal import Decimal

from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import Venue


SCHWAB = "SCHWAB"
SCHWAB_VENUE = Venue(SCHWAB)
SCHWAB_OPTION_VENUE = Venue("OPRA")


@dataclass(slots=True)
class ParsedOpraSymbol:
    """
    Structured OPRA symbol fields.
    """

    underlying: str
    expiration: datetime
    option_kind: OptionKind
    strike: Decimal


def parse_opra_symbol(symbol: str) -> ParsedOpraSymbol:
    """
    Parse an OPRA symbol into its components.

    Parameters
    ----------
    symbol : str
        The raw OPRA symbol (e.g. ``AAPL211217C00150000``).

    Returns
    -------
    ParsedOpraSymbol

    Raises
    ------
    ValueError
        If ``symbol`` does not conform to the expected OPRA format.

    """
    if len(symbol) <= 15:
        raise ValueError(f"OPRA symbol too short: {symbol}")

    underlying = symbol[:-15].strip()
    if not underlying:
        raise ValueError(f"OPRA symbol missing underlying: {symbol}")

    date_part = symbol[-15:-9]
    option_flag = symbol[-9]
    strike_part = symbol[-8:]

    try:
        year = 2000 + int(date_part[0:2])
        month = int(date_part[2:4])
        day = int(date_part[4:6])
    except ValueError as exc:
        raise ValueError(f"Invalid OPRA expiration in symbol: {symbol}") from exc

    try:
        option_kind = OptionKind.CALL if option_flag.upper() == "C" else OptionKind.PUT
    except Exception as exc:
        raise ValueError(f"Invalid OPRA option flag in symbol: {symbol}") from exc

    try:
        strike = Decimal(int(strike_part)) / Decimal(1000)
    except ValueError as exc:
        raise ValueError(f"Invalid OPRA strike in symbol: {symbol}") from exc

    expiration = datetime(year, month, day, tzinfo=UTC)
    return ParsedOpraSymbol(
        underlying=underlying,
        expiration=expiration,
        option_kind=option_kind,
        strike=strike,
    )


__all__ = [
    "SCHWAB",
    "SCHWAB_OPTION_VENUE",
    "SCHWAB_VENUE",
    "ParsedOpraSymbol",
    "parse_opra_symbol",
]
