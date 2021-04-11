# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime
from decimal import Decimal
import itertools
from typing import List, Optional, Union

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction
import pandas as pd

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.util import hash_json
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderBookOperationType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import InstrumentClosePrice
from nautilus_trader.model.events import InstrumentStatusEvent
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instrument import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from nautilus_trader.model.orderbook.book import OrderBookOperation
from nautilus_trader.model.orderbook.book import OrderBookOperations
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.tick import TradeTick


uuid_factory = UUIDFactory()


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """

    order = command.order  # type: LimitOrder
    return {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": command.strategy_id.value[:15],
        "instructions": [
            place_instruction(
                order_type="LIMIT",
                selection_id=instrument.selection_id,
                side=N2B_SIDE[order.side],
                handicap={"0.0": "0"}.get(
                    instrument.selection_handicap, instrument.selection_handicap
                ),
                limit_order=limit_order(
                    size=float(order.quantity),
                    price=float(
                        probability_to_price(probability=order.price, side=order.side)
                    ),
                    persistence_type="PERSIST",
                    time_in_force=N2B_TIME_IN_FORCE[order.time_in_force],
                    min_fill_size=0,
                ),
                customer_order_ref=order.client_order_id.value,
            )
        ],
    }


def order_update_to_betfair(
    command: UpdateOrder,
    venue_order_id: VenueOrderId,
    side: OrderSide,
    instrument: BettingInstrument,
):
    """ Convert an UpdateOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [
            replace_instruction(
                bet_id=venue_order_id.value,
                new_price=float(
                    probability_to_price(probability=command.price, side=side)
                ),
            )
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [cancel_instruction(bet_id=command.venue_order_id.value)],
    }


def betfair_account_to_account_state(
    account_detail,
    account_funds,
    event_id,
    timestamp_ns,
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail["currencyCode"])
    balance = float(account_funds["availableToBetBalance"])
    balance_locked = -float(account_funds["exposure"])
    balance_free = balance - balance_locked
    return AccountState(
        AccountId(issuer=BETFAIR_VENUE.value, identifier=account_id),
        [Money(value=balance, currency=currency)],
        [Money(value=balance_free, currency=currency)],
        [Money(value=balance_locked, currency=currency)],
        {"funds": account_funds, "detail": account_detail},
        event_id,
        timestamp_ns,
    )


def _handle_market_snapshot(selection, instrument, timestamp_ns):
    updates = []
    # Check we only have one of [best bets / depth bets / all bets]
    bid_keys = [k for k in B_BID_KINDS if k in selection] or ["atb"]
    ask_keys = [k for k in B_ASK_KINDS if k in selection] or ["atl"]
    assert len(bid_keys) <= 1
    assert len(ask_keys) <= 1

    # Orderbook Snapshot
    # TODO Clean this crap up
    if bid_keys[0] == "atb":
        bids = selection.get("atb", [])
    else:
        bids = [(p, v) for _, p, v in selection.get(bid_keys[0], [])]
    if ask_keys[0] == "atl":
        asks = selection.get("atl", [])
    else:
        asks = [(p, v) for _, p, v in selection.get(ask_keys[0], [])]
    snapshot = OrderBookSnapshot(
        level=OrderBookLevel.L2,
        instrument_id=instrument.id,
        bids=[(price_to_probability(p, OrderSide.BUY), v) for p, v in asks],
        asks=[(price_to_probability(p, OrderSide.SELL), v) for p, v in bids],
        timestamp_ns=timestamp_ns,
    )
    updates.append(snapshot)

    # Trade Ticks
    for price, volume in selection.get("trd", []):
        trade_id = hash_json((timestamp_ns, price, volume))
        tick = TradeTick(
            instrument_id=instrument.id,
            price=Price(price_to_probability(price)),
            size=Quantity(volume, precision=4),
            side=OrderSide.BUY,
            match_id=TradeMatchId(trade_id),
            timestamp_ns=timestamp_ns,
        )
        updates.append(tick)

    return updates


def _handle_market_trades(runner, instrument, timestamp_ns):
    trade_ticks = []
    for price, volume in runner.get("trd", []):
        # Betfair doesn't publish trade ids, so we make our own
        # TODO - should we use clk here for ID instead of the hash?
        trade_id = hash_json(
            data=(
                timestamp_ns,
                instrument.market_id,
                str(runner["id"]),
                str(runner.get("hc", "0.0")),
                price,
                volume,
            )
        )
        tick = TradeTick(
            instrument_id=instrument.id,
            price=Price(price_to_probability(price)),
            size=Quantity(volume, precision=4),
            side=OrderSide.BUY,
            match_id=TradeMatchId(trade_id),
            timestamp_ns=timestamp_ns,
        )
        trade_ticks.append(tick)
    return trade_ticks


def _handle_book_updates(runner, instrument, timestamp_ns):
    operations = []
    for side in B_SIDE_KINDS:
        for upd in runner.get(side, []):
            # TODO - Fix this crap
            if len(upd) == 3:
                _, price, volume = upd
            else:
                price, volume = upd
            operations.append(
                OrderBookOperation(
                    op_type=OrderBookOperationType.DELETE
                    if volume == 0
                    else OrderBookOperationType.UPDATE,
                    order=Order(
                        price=price_to_probability(
                            price, side=B2N_MARKET_STREAM_SIDE[side]
                        ),
                        volume=volume,
                        side=B2N_MARKET_STREAM_SIDE[side],
                    ),
                    timestamp_ns=timestamp_ns,
                )
            )
    if operations:
        ob_update = OrderBookOperations(
            level=OrderBookLevel.L2,
            instrument_id=instrument.id,
            ops=operations,
            timestamp_ns=timestamp_ns,
        )
        return [ob_update]
    else:
        return []


def _handle_market_close(runner, instrument, timestamp_ns):
    if runner["status"] in ("LOSER", "REMOVED"):
        close_price = InstrumentClosePrice(
            instrument_id=instrument.id,
            close_price=Price(0.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    elif runner["status"] == "WINNER":
        close_price = InstrumentClosePrice(
            instrument_id=instrument.id,
            close_price=Price(1.0, precision=4),
            close_type=InstrumentCloseType.EXPIRED,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    else:
        raise ValueError(f"Unknown runner close status: {runner['status']}")
    return [close_price]


def _handle_instrument_status(market, instrument, timestamp_ns):
    market_def = market.get("marketDefinition", {})
    if "status" not in market_def:
        return []
    if market_def["status"] == "OPEN" and not market_def["inPlay"]:
        status = InstrumentStatusEvent(
            instrument_id=instrument.id,
            status=InstrumentStatus.PRE_OPEN,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    elif market_def["status"] == "OPEN" and market_def["inPlay"]:
        status = InstrumentStatusEvent(
            instrument_id=instrument.id,
            status=InstrumentStatus.OPEN,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    elif market_def["status"] == "SUSPENDED":
        status = InstrumentStatusEvent(
            instrument_id=instrument.id,
            status=InstrumentStatus.PAUSE,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    elif market_def["status"] == "CLOSED":
        status = InstrumentStatusEvent(
            instrument_id=instrument.id,
            status=InstrumentStatus.CLOSED,
            event_id=uuid_factory.generate(),
            timestamp_ns=timestamp_ns,
        )
    else:
        raise ValueError("Unknown market status")
    return [status]


def _handle_market_runners_status(self, market, timestamp_ns):
    updates = []

    for runner in market.get("marketDefinition", {}).get("runners", []):
        kw = dict(
            market_id=market["id"],
            selection_id=str(runner["id"]),
            handicap=str(runner.get("hc") or "0.0"),
        )
        instrument = self.instrument_provider().get_betting_instrument(**kw)
        if not instrument:
            continue
        updates.extend(
            _handle_instrument_status(
                market=market, instrument=instrument, timestamp_ns=timestamp_ns
            )
        )
        if market["marketDefinition"].get("status") == "CLOSED":
            updates.extend(
                _handle_market_close(
                    runner=runner, instrument=instrument, timestamp_ns=timestamp_ns
                )
            )
    return updates


def build_market_snapshot_messages(
    self, raw
) -> List[Union[OrderBookSnapshot, InstrumentStatusEvent]]:
    updates = []
    timestamp_ns = millis_to_nanos(raw["pt"])
    for market in raw.get("mc", []):
        # Instrument Status
        updates.extend(
            _handle_market_runners_status(
                self=self, market=market, timestamp_ns=timestamp_ns
            )
        )

        # Orderbook snapshots
        if market.get("img") is True:
            market_id = market["id"]
            for (selection_id, handicap), selections in itertools.groupby(
                market.get("rc", []), lambda x: (x["id"], x.get("hc"))
            ):
                for selection in list(selections):
                    kw = dict(
                        market_id=market_id,
                        selection_id=str(selection_id),
                        handicap=str(handicap or "0.0"),
                    )
                    instrument = self.instrument_provider().get_betting_instrument(**kw)
                    updates.extend(
                        _handle_market_snapshot(
                            selection=selection,
                            instrument=instrument,
                            timestamp_ns=timestamp_ns,
                        )
                    )
    return updates


def build_market_update_messages(  # noqa TODO: cyclomatic complexity 14
    self, raw
) -> List[
    Union[OrderBookOperation, TradeTick, InstrumentStatusEvent, InstrumentClosePrice]
]:
    updates = []
    timestamp_ns = millis_to_nanos(raw["pt"])
    for market in raw.get("mc", []):
        updates.extend(
            _handle_market_runners_status(
                self=self, market=market, timestamp_ns=timestamp_ns
            )
        )
        for runner in market.get("rc", []):
            kw = dict(
                market_id=market["id"],
                selection_id=str(runner["id"]),
                handicap=str(runner.get("hc") or "0.0"),
            )
            instrument = self.instrument_provider().get_betting_instrument(**kw)
            if not instrument:
                continue
            updates.extend(
                _handle_book_updates(
                    runner=runner, instrument=instrument, timestamp_ns=timestamp_ns
                )
            )
            updates.extend(
                _handle_market_trades(
                    runner=runner, instrument=instrument, timestamp_ns=timestamp_ns
                )
            )

    return updates


def on_market_update(self, update: dict):
    if update.get("ct") == "HEARTBEAT":
        # TODO - Should we send out heartbeats
        return []
    for mc in update.get("mc", []):
        if mc.get("img"):
            return build_market_snapshot_messages(self, update)
        else:
            return build_market_update_messages(self, update)
    return []


# TODO - Need to handle pagination > 1000 orders
async def generate_order_status_report(self, order) -> Optional[OrderStatusReport]:
    return [
        OrderStatusReport(
            client_order_id=ClientOrderId(),
            venue_order_id=VenueOrderId(),
            order_state=OrderState(),
            filled_qty=Quantity(),
            timestamp_ns=millis_to_nanos(),
        )
        for order in self.client().betting.list_current_orders()["currentOrders"]
    ]


async def generate_trades_list(
    self, venue_order_id: VenueOrderId, symbol: Symbol, since: datetime = None
) -> List[ExecutionReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    timestamp_ns = millis_to_nanos(pd.Timestamp(fill["lastMatchedDate"]).timestamp())
    return [
        ExecutionReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            venue_order_id=VenueOrderId(fill["betId"]),
            execution_id=ExecutionId(fill["lastMatchedDate"]),
            last_qty=Decimal(fill["sizeSettled"]),
            last_px=Decimal(fill["priceMatched"]),
            commission_amount=None,  # Can be None
            commission_currency=None,  # Can be None
            liquidity_side=LiquiditySide.NONE,
            execution_ns=timestamp_ns,
            timestamp_ns=timestamp_ns,
        )
    ]
