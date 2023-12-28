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

from __future__ import annotations

from typing import Any

import msgspec

from nautilus_trader.adapters.databento.enums import DatabentoStatisticType
from nautilus_trader.adapters.databento.enums import DatabentoStatisticUpdateAction
from nautilus_trader.core.data import Data
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import order_side_from_str
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


Dataset = str
PublisherId = int


class DatabentoPublisher(msgspec.Struct, frozen=True):
    """
    Represents a Databento publisher including dataset name and venue.
    """

    publisher_id: int
    dataset: str
    venue: str
    description: str


class DatabentoImbalance(Data):
    """
    Represents an auction imbalance.

    This data type includes the populated data fields provided by `Databento`, except for
    the `publisher_id` and `instrument_id` integers.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the imbalance data.
    ref_price : Price
        The reference price at which the imbalance shares are calculated.
    cont_book_clr_price : Price
        The hypothetical auction-clearing price for both cross and continuous orders.
    auct_interest_clr_price : Price
        The hypothetical auction-clearing price for cross orders only.
    paired_qty : Quantity
        The quantity of shares which are eligible to be matched at `ref_price`.
    total_imbalance_qty : Quantity
        The quantity of shares which are not paired at `ref_price`.
    side : OrderSide
        The market side of the `total_imbalance_qty` (can be `NO_ORDER_SIDE`).
    significant_imbalance : str
        A venue-specific character code. For Nasdaq, contains the raw Price Variation Indicator.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred (Databento `ts_recv`).
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    References
    ----------
    https://docs.databento.com/knowledge-base/new-users/fields-by-schema/imbalance-imbalance

    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ref_price: Price,
        cont_book_clr_price: Price,
        auct_interest_clr_price: Price,
        paired_qty: Quantity,
        total_imbalance_qty: Quantity,
        side: OrderSide,
        significant_imbalance: str,
        ts_event: int,
        ts_init: int,
    ) -> None:
        self.instrument_id = instrument_id
        self.ref_price = ref_price
        self.cont_book_clr_price = cont_book_clr_price
        self.auct_interest_clr_price = auct_interest_clr_price
        self.paired_qty = paired_qty
        self.total_imbalance_qty = total_imbalance_qty
        self.side = side
        self.significant_imbalance = significant_imbalance
        self._ts_event = ts_event  # Required for `Data` base class
        self._ts_init = ts_init  # Required for `Data` base class

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, DatabentoImbalance):
            return False
        return self.instrument_id == other.instrument_id and self.ts_event == other.ts_event

    def __hash__(self) -> int:
        return hash((self.instrument_id, self.ts_event))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"ref_price={self.ref_price}, "
            f"cont_book_clr_price={self.cont_book_clr_price}, "
            f"auct_interest_clr_price={self.auct_interest_clr_price}, "
            f"paired_qty={self.paired_qty}, "
            f"total_imbalance_qty={self.total_imbalance_qty}, "
            f"side={order_side_to_str(self.side)}, "
            f"significant_imbalance={self.significant_imbalance}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred (Databento
        `ts_recv`).

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

    @staticmethod
    def from_dict(values: dict[str, Any]) -> DatabentoImbalance:
        """
        Return `DatabentoImbalance` parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        DatabentoImbalance

        """
        return DatabentoImbalance(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            ref_price=Price.from_str(values["ref_price"]),
            cont_book_clr_price=Price.from_str(values["cont_book_clr_price"]),
            auct_interest_clr_price=Price.from_str(values["auct_interest_clr_price"]),
            paired_qty=Quantity.from_str(values["paired_qty"]),
            total_imbalance_qty=Quantity.from_str(values["total_imbalance_qty"]),
            side=order_side_from_str(values["side"]),
            significant_imbalance=values["significant_imbalance"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: DatabentoImbalance) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "ref_price": str(obj.ref_price),
            "cont_book_clr_price": str(obj.cont_book_clr_price),
            "auct_interest_clr_price": str(obj.auct_interest_clr_price),
            "paired_qty": str(obj.paired_qty),
            "total_imbalance_qty": str(obj.total_imbalance_qty),
            "side": order_side_to_str(obj.side),
            "significant_imbalance": obj.significant_imbalance,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }


class DatabentoStatistics(Data):
    """
    Represents a statistics message.

    This data type includes the populated data fields provided by `Databento`, except for
    the `publisher_id` and `instrument_id` integers.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the statistics message.
    stat_type : DatabentoStatisticType
        The type of statistic value contained in the message.
    update_action : DatabentoStatisticUpdateAction
        Indicates if the statistic is newly added (1) or deleted (2).
        (Deleted is only used with some stat_types).
    price : Price, optional
        The statistics price.
    quantity : Quantity, optional
        The value for non-price statistics.
    channel_id : int
        The channel ID within the venue.
    stat_flags : int
        Additional flags associated with certain stat types.
    sequence : int
        The message sequence number assigned at the venue.
    ts_ref : uint64_t
        The UNIX timestamp (nanoseconds) Databento `ts_ref` reference timestamp).
    ts_in_delta : int32_t
        The matching-engine-sending timestamp expressed as the number of nanoseconds before the Databento `ts_recv`.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred (Databento `ts_recv`).
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    References
    ----------
    https://docs.databento.com/knowledge-base/new-users/fields-by-schema/statistics-statistics

    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        stat_type: DatabentoStatisticType,
        update_action: DatabentoStatisticUpdateAction,
        price: Price | None,
        quantity: Quantity | None,
        channel_id: int,
        stat_flags: int,
        sequence: int,
        ts_ref: int,
        ts_in_delta: int,
        ts_event: int,
        ts_init: int,
    ) -> None:
        self.instrument_id = instrument_id
        self.stat_type = stat_type
        self.update_action = update_action
        self.price = price
        self.quantity = quantity
        self.channel_id = channel_id
        self.stat_flags = stat_flags
        self.sequence = sequence
        self.ts_ref = ts_ref
        self.ts_in_delta = ts_in_delta
        self._ts_event = ts_event  # Required for `Data` base class
        self._ts_init = ts_init  # Required for `Data` base class

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, DatabentoStatistics):
            return False
        return self.instrument_id == other.instrument_id and self.ts_event == other.ts_event

    def __hash__(self) -> int:
        return hash((self.instrument_id, self.ts_event))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id}, "
            f"stat_type={self.stat_type}, "
            f"update_action={self.update_action}, "
            f"price={self.price}, "
            f"quantity={self.quantity}, "
            f"channel_id={self.channel_id}, "
            f"stat_flags={self.stat_flags}, "
            f"sequence={self.sequence}, "
            f"ts_ref={self.ts_ref}, "
            f"ts_in_delta={self.ts_in_delta}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred (Databento
        `ts_recv`).

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

    @staticmethod
    def from_dict(values: dict[str, Any]) -> DatabentoStatistics:
        """
        Return `DatabentoStatistics` parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        DatabentoStatistics

        """
        price: str | None = values["price"]
        quantity: str | None = values["quantity"]

        return DatabentoStatistics(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            stat_type=DatabentoStatisticType(values["stat_type"]),
            update_action=DatabentoStatisticUpdateAction(values["update_action"]),
            price=Price.from_str(price) if price is not None else None,
            quantity=Quantity.from_str(quantity) if quantity is not None else None,
            channel_id=values["channel_id"],
            stat_flags=values["stat_flags"],
            sequence=values["sequence"],
            ts_ref=values["ts_ref"],
            ts_in_delta=values["ts_in_delta"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: DatabentoStatistics) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "stat_type": obj.stat_type.value,
            "update_action": obj.update_action.value,
            "price": str(obj.price) if obj.price is not None else None,
            "quantity": str(obj.quantity) if obj.quantity is not None else None,
            "channel_id": obj.channel_id,
            "stat_flags": obj.stat_flags,
            "sequence": obj.sequence,
            "ts_ref": obj.ts_ref,
            "ts_in_delta": obj.ts_in_delta,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }
