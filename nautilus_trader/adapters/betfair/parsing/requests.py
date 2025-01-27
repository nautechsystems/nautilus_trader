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

import hashlib
from functools import lru_cache
from typing import Literal

import msgspec
import pandas as pd
from betfair_parser.spec.accounts.type_definitions import AccountDetailsResponse
from betfair_parser.spec.accounts.type_definitions import AccountFundsResponse
from betfair_parser.spec.betting.enums import PersistenceType
from betfair_parser.spec.betting.orders import CancelOrders
from betfair_parser.spec.betting.orders import PlaceInstruction
from betfair_parser.spec.betting.orders import PlaceOrders
from betfair_parser.spec.betting.orders import ReplaceInstruction
from betfair_parser.spec.betting.orders import ReplaceOrders
from betfair_parser.spec.betting.type_definitions import CancelInstruction
from betfair_parser.spec.betting.type_definitions import CurrentOrderSummary
from betfair_parser.spec.betting.type_definitions import LimitOnCloseOrder
from betfair_parser.spec.betting.type_definitions import LimitOrder
from betfair_parser.spec.betting.type_definitions import MarketOnCloseOrder
from betfair_parser.spec.common import BetId
from betfair_parser.spec.common import CustomerOrderRef
from betfair_parser.spec.common import OrderSide as BetOrderSide
from betfair_parser.spec.common import OrderStatus as BetfairOrderStatus
from betfair_parser.spec.common import OrderType
from betfair_parser.spec.streaming import Order as BetfairOrder

from nautilus_trader.adapters.betfair.common import B2N_ORDER_TYPE
from nautilus_trader.adapters.betfair.common import B2N_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import BETFAIR_FLOAT_TO_PRICE
from nautilus_trader.adapters.betfair.common import MAX_BET_PRICE
from nautilus_trader.adapters.betfair.common import MIN_BET_PRICE
from nautilus_trader.adapters.betfair.common import N2B_PERSISTENCE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import OrderSideParser
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing.common import min_fill_size
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce as NautilusTimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.betting import null_handicap
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder as NautilusLimitOrder
from nautilus_trader.model.orders import MarketOrder as NautilusMarketOrder


def make_customer_order_ref(client_order_id: ClientOrderId) -> CustomerOrderRef:
    """
    User-set reference for the order.

    From the Betfair docs:
    An optional reference customers can set to identify instructions. No validation will be done on uniqueness and the
    string is limited to 32 characters. If an empty string is provided it will be treated as null.

    """
    return client_order_id.value[:32]


def nautilus_limit_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    assert isinstance(command.order, NautilusLimitOrder)
    instructions = PlaceInstruction(
        order_type=OrderType.LIMIT,
        selection_id=int(instrument.selection_id),
        handicap=(
            instrument.selection_handicap
            if instrument.selection_handicap != null_handicap()
            else None
        ),
        side=OrderSideParser.to_betfair(command.order.side),
        limit_order=LimitOrder(
            price=command.order.price.as_double(),
            size=command.order.quantity.as_double(),
            persistence_type=N2B_PERSISTENCE.get(
                command.order.time_in_force,
                PersistenceType.LAPSE,
            ),
            time_in_force=N2B_TIME_IN_FORCE.get(command.order.time_in_force),
            min_fill_size=min_fill_size(command.order.time_in_force),
        ),
        customer_order_ref=make_customer_order_ref(
            client_order_id=command.order.client_order_id,
        ),
    )
    return instructions


def nautilus_limit_on_close_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    assert isinstance(command.order, NautilusLimitOrder)
    instructions = PlaceInstruction(
        order_type=OrderType.LIMIT_ON_CLOSE,
        selection_id=int(instrument.selection_id),
        handicap=(
            instrument.selection_handicap
            if instrument.selection_handicap != null_handicap()
            else None
        ),
        side=OrderSideParser.to_betfair(command.order.side),
        limit_on_close_order=LimitOnCloseOrder(
            price=command.order.price.as_double(),
            liability=command.order.quantity.as_double(),
        ),
        customer_order_ref=make_customer_order_ref(
            client_order_id=command.order.client_order_id,
        ),
    )
    return instructions


def nautilus_market_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    assert isinstance(command.order, NautilusMarketOrder)
    price = MIN_BET_PRICE if command.order.side == OrderSide.BUY else MAX_BET_PRICE
    instructions = PlaceInstruction(
        order_type=OrderType.LIMIT,
        selection_id=int(instrument.selection_id),
        handicap=(
            instrument.selection_handicap
            if instrument.selection_handicap != null_handicap()
            else None
        ),
        side=OrderSideParser.to_betfair(command.order.side),
        limit_order=LimitOrder(
            price=price.as_double(),
            size=command.order.quantity.as_double(),
            persistence_type=N2B_PERSISTENCE.get(
                command.order.time_in_force,
                PersistenceType.LAPSE,
            ),
            time_in_force=N2B_TIME_IN_FORCE.get(command.order.time_in_force),
            min_fill_size=min_fill_size(command.order.time_in_force),
        ),
        customer_order_ref=make_customer_order_ref(
            client_order_id=command.order.client_order_id,
        ),
    )
    return instructions


def nautilus_market_on_close_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    assert isinstance(command.order, NautilusMarketOrder)
    instructions = PlaceInstruction(
        order_type=OrderType.MARKET_ON_CLOSE,
        selection_id=int(instrument.selection_id),
        handicap=(
            instrument.selection_handicap
            if instrument.selection_handicap != null_handicap()
            else None
        ),
        side=OrderSideParser.to_betfair(command.order.side),
        market_on_close_order=MarketOnCloseOrder(
            liability=command.order.quantity.as_double(),
        ),
        customer_order_ref=make_customer_order_ref(
            client_order_id=command.order.client_order_id,
        ),
    )
    return instructions


def nautilus_order_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    if isinstance(command.order, NautilusLimitOrder):
        if command.order.time_in_force in (
            NautilusTimeInForce.AT_THE_OPEN,
            NautilusTimeInForce.AT_THE_CLOSE,
        ):
            return nautilus_limit_on_close_to_place_instructions(
                command=command,
                instrument=instrument,
            )
        else:
            return nautilus_limit_to_place_instructions(command=command, instrument=instrument)
    elif isinstance(command.order, NautilusMarketOrder):
        if command.order.time_in_force in (
            NautilusTimeInForce.AT_THE_OPEN,
            NautilusTimeInForce.AT_THE_CLOSE,
        ):
            return nautilus_market_on_close_to_place_instructions(
                command=command,
                instrument=instrument,
            )
        else:
            return nautilus_market_to_place_instructions(command=command, instrument=instrument)
    else:
        raise TypeError(f"Unknown order type: {type(command.order)}")


def order_submit_to_place_order_params(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceOrders:
    """
    Convert a SubmitOrder command into the data required by BetfairClient.
    """
    return PlaceOrders.with_params(
        market_id=instrument.market_id,
        customer_ref=create_customer_ref(command),
        customer_strategy_ref=create_customer_strategy_ref(
            trader_id=command.trader_id.value,
            strategy_id=command.strategy_id.value,
        ),
        instructions=[nautilus_order_to_place_instructions(command, instrument)],
    )


def order_update_to_replace_order_params(
    command: ModifyOrder,
    venue_order_id: VenueOrderId,
    instrument: BettingInstrument,
) -> ReplaceOrders:
    """
    Convert an ModifyOrder command into the data required by BetfairClient.
    """
    return ReplaceOrders.with_params(
        market_id=instrument.market_id,
        customer_ref=create_customer_ref(command),
        instructions=[
            ReplaceInstruction(
                bet_id=BetId(venue_order_id.value),
                new_price=command.price.as_double(),
            ),
        ],
    )


def order_update_to_cancel_order_params(
    command: CancelOrder,
    instrument: BettingInstrument,
    size_reduction,
) -> CancelOrders:
    """
    Convert a CancelOrder command into the data required by BetfairClient.
    """
    return CancelOrders.with_params(
        market_id=instrument.market_id,
        instructions=[
            CancelInstruction(
                bet_id=BetId(command.venue_order_id.value),
                size_reduction=size_reduction,
            ),
        ],
        customer_ref=create_customer_ref(command),
    )


def order_cancel_to_cancel_order_params(
    command: CancelOrder,
    instrument: BettingInstrument,
) -> CancelOrders:
    """
    Convert a CancelOrder command into the data required by BetfairClient.
    """
    return CancelOrders.with_params(
        market_id=instrument.market_id,
        instructions=[CancelInstruction(bet_id=BetId(command.venue_order_id.value))],
        customer_ref=create_customer_ref(command),
    )


def order_cancel_all_to_betfair(instrument: BettingInstrument) -> dict[str, str]:
    """
    Convert a CancelAllOrders command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
    }


def betfair_account_to_account_state(
    account_detail: AccountDetailsResponse,
    account_funds: AccountFundsResponse,
    event_id,
    ts_event,
    ts_init,
    reported,
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail.currency_code)
    free = float(account_funds.available_to_bet_balance)
    locked = -float(account_funds.exposure)
    total = free + locked
    return AccountState(
        account_id=AccountId(f"{BETFAIR_VENUE.value}-{account_id}"),
        account_type=AccountType.BETTING,
        base_currency=currency,
        reported=reported,
        balances=[
            AccountBalance(
                total=Money(total, currency),
                locked=Money(locked, currency),
                free=Money(free, currency),
            ),
        ],
        margins=[],
        info={"funds": account_funds, "detail": account_detail},
        event_id=event_id,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def bet_to_fill_report(
    order: CurrentOrderSummary,
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
    base_currency: Currency,
    ts_init,
    report_id,
) -> FillReport:
    ts_event = pd.Timestamp(order.matched_date).value
    trade_id = current_order_summary_to_trade_id(order)
    return FillReport(
        client_order_id=client_order_id,
        instrument_id=instrument_id,
        account_id=account_id,
        venue_order_id=venue_order_id,
        venue_position_id=None,  # Can be None
        order_side=OrderSideParser.to_nautilus(order.side),
        trade_id=trade_id,
        last_qty=Quantity(order.size_matched, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(order.price_size.price, BETFAIR_PRICE_PRECISION),
        commission=Money(0.0, base_currency),
        liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
        report_id=report_id,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def bet_to_order_status_report(
    order: CurrentOrderSummary,
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
    ts_init,
    report_id,
) -> OrderStatusReport:
    if order.price_size.size != 0.0:
        qty = Quantity(order.price_size.size, BETFAIR_QUANTITY_PRECISION)
        fill_qty = Quantity(order.size_matched, BETFAIR_QUANTITY_PRECISION)
    elif order.bsp_liability != 0.0:
        size = (
            order.bsp_liability / order if order.side == BetOrderSide.BACK else order.bsp_liability
        )
        qty = Quantity(size, BETFAIR_QUANTITY_PRECISION)
        fill_qty = Quantity(size, BETFAIR_QUANTITY_PRECISION)
    else:
        raise ValueError(f"Unknown order size {order.price_size.size=}, {order.bsp_liability=}")
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=venue_order_id,
        client_order_id=client_order_id,
        order_side=OrderSideParser.to_nautilus(order.side),
        order_type=B2N_ORDER_TYPE[order.order_type],
        contingency_type=ContingencyType.NO_CONTINGENCY,
        time_in_force=B2N_TIME_IN_FORCE[order.persistence_type],
        order_status=determine_order_status(order),
        price=BETFAIR_FLOAT_TO_PRICE[order.price_size.price],
        quantity=qty,
        filled_qty=fill_qty,
        report_id=report_id,
        ts_accepted=dt_to_unix_nanos(pd.Timestamp(order.placed_date)),
        ts_triggered=0,
        ts_last=dt_to_unix_nanos(pd.Timestamp(order.matched_date)) if order.matched_date else 0,
        ts_init=ts_init,
    )


def determine_order_status(order: CurrentOrderSummary) -> OrderStatus:
    order_size = order.price_size.size

    if order.status == BetfairOrderStatus.EXECUTION_COMPLETE:
        if order_size == order.size_matched:
            return OrderStatus.FILLED
        elif order.size_cancelled and order.size_cancelled > 0.0:
            return OrderStatus.CANCELED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif order.status == BetfairOrderStatus.EXECUTABLE:
        if order.size_matched == 0.0:
            return OrderStatus.ACCEPTED
        elif order.size_matched and order.size_matched > 0.0:
            return OrderStatus.PARTIALLY_FILLED
    elif order.status == BetfairOrderStatus.EXPIRED:
        # Time in force requirement resulted in a cancel
        if order.size_matched == 0.0:
            return OrderStatus.CANCELED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif order.status == BetfairOrderStatus.PENDING:
        # Accepted, but yet to be processed
        return OrderStatus.ACCEPTED

    raise ValueError(f"Unknown order status {order.status=}")


def create_customer_ref(command: SubmitOrder | ModifyOrder | CancelOrder) -> str:
    """
    Create a customer reference for the betfair API from order command.

    From betfair docs (https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/placeOrders):
        Optional parameter allowing the client to pass a unique string (up to 32 chars) that is used to de-dupe
        mistaken re-submissions.   customerRef can contain: upper/lower chars, digits, chars : - . _ + * : ; ~ only.

        Please note: There is a time window associated with the de-duplication of duplicate submissions which is 60 seconds.

        NB: This field does not persist into the placeOrders response/Order Stream API and should not be confused with
        customerOrderRef, which is separate field that can be sent in the PlaceInstruction.

    Parameters
    ----------
    command: SubmitOrder | ModifyOrder | CancelOrder
        The order command

    Returns
    -------
    str

    """
    return command.id.value.replace("-", "")[:32]


@lru_cache
def create_customer_strategy_ref(trader_id: str, strategy_id: str) -> str:
    """
    Betfair allow setting a strategy reference, limited to 15 chars. Produce a hash to
    use as a strategy reference in the place order API.

    From the docs:

    "An optional reference customers can use to specify which strategy has sent the order.
    The reference will be returned on order change messages through the stream API. The string is
    limited to 15 characters. If an empty string is provided it will be treated as null."

    Produce a hash to use as a strategy ID in the place order API.

    https://docs.developer.betfair.com/display/1smk3cen4v3lu3yomq5qye0ni/placeOrders


    Parameters
    ----------
    trader_id: str
        The trader ID
    strategy_id: str
        The strategy ID

    Returns
    -------
    str

    """
    data = {
        "trader_id": trader_id,
        "strategy_id": strategy_id,
    }
    return hashlib.shake_256(msgspec.json.encode(data)).hexdigest(8)[:15]


def hashed_trade_id(
    bet_id: BetId,
    price: float,
    size: float,
    side: Literal["B", "L"],
    persistence_type: Literal["L", "P", "MOC"],
    order_type: Literal["L", "MOC", "LOC"],
    placed_date: int,
    matched_date: int | None = None,
    average_price_matched: float | None = None,
    size_matched: float | None = None,
) -> TradeId:
    data: bytes = msgspec.json.encode(
        (
            bet_id,
            price,
            size,
            side,
            persistence_type,
            order_type,
            placed_date,
            matched_date,
            average_price_matched,
            size_matched,
        ),
    )
    return TradeId(hashlib.shake_256(msgspec.json.encode(data)).hexdigest(18))


def order_to_trade_id(uo: BetfairOrder) -> TradeId:
    return hashed_trade_id(
        bet_id=uo.id,
        price=uo.p,
        size=uo.s,
        side=uo.side,
        persistence_type=uo.pt,
        order_type=uo.ot,
        placed_date=uo.pd,
        matched_date=uo.md,
        average_price_matched=uo.avp,
        size_matched=uo.sm,
    )


def current_order_summary_to_trade_id(order: CurrentOrderSummary) -> TradeId:
    return hashed_trade_id(
        bet_id=order.bet_id,
        price=order.price_size.price,
        size=order.price_size.size,
        side=order.side.value[0],
        persistence_type=order.persistence_type.value,
        order_type=order.order_type.value,
        placed_date=order.placed_date,
        matched_date=order.matched_date,
        average_price_matched=order.average_price_matched,
        size_matched=order.size_matched,
    )
