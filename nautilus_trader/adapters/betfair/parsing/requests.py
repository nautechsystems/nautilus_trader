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

from datetime import datetime
from functools import lru_cache
from typing import Optional

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
from betfair_parser.spec.common import CustomerOrderRef
from betfair_parser.spec.common import OrderStatus as BetfairOrderStatus
from betfair_parser.spec.common import OrderType

from nautilus_trader.adapters.betfair.common import B2N_ORDER_SIDE
from nautilus_trader.adapters.betfair.common import B2N_ORDER_TYPE
from nautilus_trader.adapters.betfair.common import B2N_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import BETFAIR_FLOAT_TO_PRICE
from nautilus_trader.adapters.betfair.common import MAX_BET_PRICE
from nautilus_trader.adapters.betfair.common import MIN_BET_PRICE
from nautilus_trader.adapters.betfair.common import N2B_PERSISTENCE
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
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
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder as NautilusLimitOrder
from nautilus_trader.model.orders import MarketOrder as NautilusMarketOrder


def make_custom_order_ref(
    client_order_id: ClientOrderId,
    strategy_id: StrategyId,
) -> CustomerOrderRef:
    """
    Remove the strategy name from customer_order_ref; it has a limited size and don't
    control what length the strategy might be or what characters users might append.
    """
    return client_order_id.value.rsplit("-" + strategy_id.get_tag(), maxsplit=1)[0]


def nautilus_limit_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    assert isinstance(command.order, NautilusLimitOrder)
    instructions = PlaceInstruction(
        order_type=OrderType.LIMIT,
        selection_id=int(instrument.selection_id),
        handicap=instrument.selection_handicap,
        side=N2B_SIDE[command.order.side],
        limit_order=LimitOrder(
            price=command.order.price.as_double(),
            size=command.order.quantity.as_double(),
            persistence_type=N2B_PERSISTENCE.get(
                command.order.time_in_force,
                PersistenceType.LAPSE,
            ),
            time_in_force=N2B_TIME_IN_FORCE.get(command.order.time_in_force),
        ),
        customer_order_ref=make_custom_order_ref(
            client_order_id=command.order.client_order_id,
            strategy_id=command.strategy_id,
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
        handicap=instrument.selection_handicap,
        side=N2B_SIDE[command.order.side],
        limit_on_close_order=LimitOnCloseOrder(
            price=command.order.price.as_double(),
            liability=command.order.quantity.as_double(),
        ),
        customer_order_ref=make_custom_order_ref(
            client_order_id=command.order.client_order_id,
            strategy_id=command.strategy_id,
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
        handicap=instrument.selection_handicap,
        side=N2B_SIDE[command.order.side],
        limit_order=LimitOrder(
            price=price.as_double(),
            size=command.order.quantity.as_double(),
            persistence_type=N2B_PERSISTENCE.get(
                command.order.time_in_force,
                PersistenceType.LAPSE,
            ),
            time_in_force=N2B_TIME_IN_FORCE.get(command.order.time_in_force),
        ),
        customer_order_ref=make_custom_order_ref(
            client_order_id=command.order.client_order_id,
            strategy_id=command.strategy_id,
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
        handicap=instrument.selection_handicap,
        side=N2B_SIDE[command.order.side],
        market_on_close_order=MarketOnCloseOrder(
            liability=command.order.quantity.as_double(),
        ),
        customer_order_ref=make_custom_order_ref(
            client_order_id=command.order.client_order_id,
            strategy_id=command.strategy_id,
        ),
    )
    return instructions


def nautilus_order_to_place_instructions(
    command: SubmitOrder,
    instrument: BettingInstrument,
) -> PlaceInstruction:
    if isinstance(command.order, NautilusLimitOrder):
        if command.order.time_in_force == NautilusTimeInForce.AT_THE_OPEN:
            return nautilus_limit_on_close_to_place_instructions(
                command=command,
                instrument=instrument,
            )
        else:
            return nautilus_limit_to_place_instructions(command=command, instrument=instrument)
    elif isinstance(command.order, NautilusMarketOrder):
        if command.order.time_in_force == NautilusTimeInForce.AT_THE_OPEN:
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
        customer_ref=command.id.value.replace(
            "-",
            "",
        ),  # Used to de-dupe orders on betfair server side
        customer_strategy_ref=command.strategy_id.value[:15],
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
        customer_ref=command.id.value.replace("-", ""),
        instructions=[
            ReplaceInstruction(
                bet_id=venue_order_id.value,
                new_price=command.price.as_double(),
            ),
        ],
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
        instructions=[CancelInstruction(bet_id=command.venue_order_id.value)],
        customer_ref=command.id.value.replace("-", ""),
    )


def order_cancel_all_to_betfair(instrument: BettingInstrument):
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
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail.currency_code)
    balance = float(account_funds.available_to_bet_balance)
    locked = -float(account_funds.exposure)
    free = balance - locked
    return AccountState(
        account_id=AccountId(f"{BETFAIR_VENUE.value}-{account_id}"),
        account_type=AccountType.BETTING,
        base_currency=currency,
        reported=False,
        balances=[
            AccountBalance(
                total=Money(balance, currency),
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


async def generate_trades_list(
    self,
    venue_order_id: VenueOrderId,
    symbol: Symbol,
    since: Optional[datetime] = None,
) -> list[TradeReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    ts_event = pd.Timestamp(fill["lastMatchedDate"]).value
    return [
        TradeReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            instrument_id=None,  # TODO: Needs this
            account_id=None,  # TODO: Needs this
            venue_order_id=VenueOrderId(fill["betId"]),
            venue_position_id=None,  # Can be None
            order_side=OrderSide.NO_ORDER_SIDE,  # TODO: Stub value
            trade_id=TradeId(fill["lastMatchedDate"]),
            last_qty=Quantity.from_str(str(fill["sizeSettled"])),  # TODO: Incorrect precision?
            last_px=Price.from_str(str(fill["priceMatched"])),  # TODO: Incorrect precision?
            commission=None,  # Can be None
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            report_id=UUID4(),
            ts_event=ts_event,
            ts_init=ts_event,
        ),
    ]


@lru_cache(None)
def parse_handicap(x) -> Optional[str]:
    """
    Ensure consistent parsing of the various handicap sources we get.
    """
    if x in (None, ""):
        return "0.0"
    if isinstance(x, (int, str)):
        return str(float(x))
    elif isinstance(x, float):
        return str(x)
    else:
        raise TypeError(f"Unexpected type ({type(x)}) for handicap: {x}")


def bet_to_order_status_report(
    order: CurrentOrderSummary,
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
    ts_init,
    report_id,
) -> OrderStatusReport:
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=venue_order_id,
        client_order_id=client_order_id,
        order_side=B2N_ORDER_SIDE[order.side],
        order_type=B2N_ORDER_TYPE[order.order_type],
        contingency_type=ContingencyType.NO_CONTINGENCY,
        time_in_force=B2N_TIME_IN_FORCE[order.persistence_type],
        order_status=determine_order_status(order),
        price=BETFAIR_FLOAT_TO_PRICE[order.price_size.price],
        quantity=Quantity(order.price_size.size, BETFAIR_QUANTITY_PRECISION),
        filled_qty=Quantity(order.size_matched, BETFAIR_QUANTITY_PRECISION),
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
        elif order.size_cancelled > 0.0:
            return OrderStatus.CANCELED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif order.status == BetfairOrderStatus.EXECUTABLE:
        if order.size_matched == 0.0:
            return OrderStatus.ACCEPTED
        elif order.size_matched > 0.0:
            return OrderStatus.PARTIALLY_FILLED
    else:
        raise ValueError(f"Unknown order status {order.status=}")
