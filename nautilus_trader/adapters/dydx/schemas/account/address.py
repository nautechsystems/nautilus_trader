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
"""
Define the schemas for the GetAddress endpoint.
"""

# ruff: noqa: N815

import datetime
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.dydx.common.constants import CURRENCY_MAP
from nautilus_trader.adapters.dydx.common.enums import DYDXEnumParser
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualPositionStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXPositionSide
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity


class DYDXPerpetualPosition(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the perpetual position.
    """

    market: str
    status: DYDXPerpetualPositionStatus
    side: DYDXPositionSide
    size: str
    maxSize: str
    entryPrice: str
    realizedPnl: str
    createdAt: datetime.datetime
    createdAtHeight: str
    sumOpen: str
    sumClose: str
    netFunding: str
    unrealizedPnl: str
    subaccountNumber: int
    exitPrice: str | None = None
    closedAt: datetime.datetime | None = None

    def base_currency(self) -> str:
        """
        Return the quote currency.
        """
        currency = self.market.split("-")[0]
        return CURRENCY_MAP.get(currency, currency)

    def quote_currency(self) -> str:
        """
        Return the quote currency.
        """
        currency = self.market.split("-")[1]
        return CURRENCY_MAP.get(currency, currency)

    def parse_margin_balance(
        self,
        margin_init: Decimal,
        margin_maint: Decimal,
        oracle_price: Decimal | None = None,
    ) -> MarginBalance:
        """
        Parse the position message into a margin balance report.
        """
        currency = Currency.from_str(self.quote_currency())

        if self.status == DYDXPerpetualPositionStatus.OPEN:
            if oracle_price is None:
                oracle_price = Decimal(self.entryPrice)

            return MarginBalance(
                initial=Money(
                    margin_init * abs(Decimal(self.size)) * oracle_price,
                    currency,
                ),
                maintenance=Money(
                    margin_maint * abs(Decimal(self.size)) * oracle_price,
                    currency,
                ),
            )

        return MarginBalance(
            initial=Money(Decimal(0), currency),
            maintenance=Money(Decimal(0), currency),
        )

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        report_id: UUID4,
        size_precision: int,
        enum_parser: DYDXEnumParser,
        ts_init: int,
    ) -> PositionStatusReport:
        """
        Parse the position message into a PositionStatusReport.
        """
        ts_last = dt_to_unix_nanos(self.createdAt)

        if self.closedAt is not None:
            ts_last = dt_to_unix_nanos(self.closedAt)

        return PositionStatusReport(
            account_id=account_id,
            instrument_id=DYDXSymbol(self.market).to_instrument_id(),
            position_side=enum_parser.parse_dydx_position_side(self.side),
            quantity=Quantity(abs(Decimal(self.size)), size_precision),
            report_id=report_id,
            ts_init=ts_init,
            ts_last=ts_last,
        )


class DYDXAssetPosition(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the asset position.
    """

    symbol: str
    side: DYDXPositionSide
    size: str
    assetId: str
    subaccountNumber: int


class DYDXSubaccount(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the schema for the subaccount response.
    """

    address: str
    subaccountNumber: int
    equity: str
    freeCollateral: str
    openPerpetualPositions: dict[str, DYDXPerpetualPosition]
    assetPositions: dict[str, DYDXAssetPosition]
    marginEnabled: bool
    updatedAtHeight: str | None = None
    latestProcessedBlockHeight: str | None = None


class DYDXSubaccountResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the address response message.
    """

    subaccount: DYDXSubaccount


class DYDXAddressResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Define the address response message.
    """

    subaccounts: list[DYDXSubaccount]
    totalTradingRewards: str
