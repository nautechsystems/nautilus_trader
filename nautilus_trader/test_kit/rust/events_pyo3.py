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

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import AccountBalance
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import AccountState
from nautilus_trader.core.nautilus_pyo3 import AccountType
from nautilus_trader.core.nautilus_pyo3 import CashAccount
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import ContingencyType
from nautilus_trader.core.nautilus_pyo3 import CryptoFuture
from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import CurrencyPair
from nautilus_trader.core.nautilus_pyo3 import Equity
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import MarketOrder
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderAccepted
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderCancelRejected
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderEmulated
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import OrderModifyRejected
from nautilus_trader.core.nautilus_pyo3 import OrderPendingCancel
from nautilus_trader.core.nautilus_pyo3 import OrderPendingUpdate
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderReleased
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderSubmitted
from nautilus_trader.core.nautilus_pyo3 import OrderTriggered
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import TriggerType
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3
from nautilus_trader.test_kit.rust.types_pyo3 import TestTypesProviderPyo3


_STUB_UUID4 = UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2")


class TestEventsProviderPyo3:
    @staticmethod
    def cash_account_state() -> AccountState:
        return AccountState(
            account_id=TestIdProviderPyo3.account_id(),
            account_type=AccountType.CASH,
            base_currency=Currency.from_str("USD"),
            balances=[
                TestTypesProviderPyo3.account_balance(),
            ],
            margins=[],
            is_reported=True,
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def cash_account_state_million_usd() -> AccountState:
        return AccountState(
            account_id=TestIdProviderPyo3.account_id(),
            account_type=AccountType.CASH,
            base_currency=Currency.from_str("USD"),
            balances=[
                TestTypesProviderPyo3.account_balance(
                    total=Money.from_str("1000000 USD"),
                    locked=Money.from_str("0 USD"),
                    free=Money.from_str("1000000 USD"),
                ),
            ],
            margins=[],
            is_reported=True,
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def cash_account_state_multi() -> AccountState:
        return AccountState(
            account_id=TestIdProviderPyo3.account_id(),
            account_type=AccountType.CASH,
            base_currency=None,
            balances=[
                AccountBalance(
                    total=Money.from_str("10 BTC"),
                    locked=Money.from_str("0 BTC"),
                    free=Money.from_str("10 BTC"),
                ),
                AccountBalance(
                    total=Money.from_str("20 ETH"),
                    locked=Money.from_str("0 ETH"),
                    free=Money.from_str("20 ETH"),
                ),
            ],
            margins=[],
            is_reported=True,
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def cash_account_state_multi_changed_btc() -> AccountState:
        return AccountState(
            account_id=TestIdProviderPyo3.account_id(),
            account_type=AccountType.CASH,
            base_currency=None,
            balances=[
                AccountBalance(
                    total=Money.from_str("9 BTC"),
                    locked=Money.from_str("0.5 BTC"),
                    free=Money.from_str("8.5 BTC"),
                ),
                AccountBalance(
                    total=Money.from_str("20 ETH"),
                    locked=Money.from_str("0 ETH"),
                    free=Money.from_str("20 ETH"),
                ),
            ],
            margins=[],
            is_reported=True,
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def margin_account_state() -> AccountState:
        return AccountState(
            account_id=TestIdProviderPyo3.account_id(),
            account_type=AccountType.MARGIN,
            base_currency=Currency.from_str("USD"),
            balances=[
                TestTypesProviderPyo3.account_balance(),
            ],
            margins=[
                TestTypesProviderPyo3.margin_balance(),
            ],
            is_reported=True,
            event_id=UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_denied_max_submit_rate() -> OrderDenied:
        return OrderDenied(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.audusd_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            reason="Exceeded MAX_ORDER_SUBMIT_RATE",
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_rejected_insufficient_margin() -> OrderRejected:
        return OrderRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.audusd_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            reason="INSUFFICIENT_MARGIN",
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_filled_buy_limit() -> OrderFilled:
        return OrderFilled(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            last_qty=Quantity.from_str("0.561000"),
            last_px=Price.from_str("15600.12445"),
            currency=Currency.from_str("USDT"),
            liquidity_side=LiquiditySide.MAKER,
            position_id=PositionId("2"),
            commission=Money.from_str("12.2 USDT"),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_initialized() -> OrderInitialized:
        return OrderInitialized(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_str("0.561000"),
            time_in_force=TimeInForce.DAY,
            post_only=True,
            reduce_only=True,
            quote_quantity=False,
            reconciliation=False,
            event_id=_STUB_UUID4,
            emulation_trigger=TriggerType.BID_ASK,
            trigger_instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            price=Price.from_str("1520.10"),
            contingency_type=ContingencyType.OTO,
            linked_order_ids=[ClientOrderId("O-2020872378424")],
            order_list_id=OrderListId("1"),
            parent_order_id=None,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            exec_spawn_id=None,
            tags=["ENTRY"],
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_triggered() -> OrderTriggered:
        return OrderTriggered(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            reconciliation=False,
        )

    @staticmethod
    def order_submitted() -> OrderSubmitted:
        return OrderSubmitted(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_emulated() -> OrderEmulated:
        return OrderEmulated(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_released() -> OrderReleased:
        return OrderReleased(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            released_price=Price.from_str("22000.0"),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_updated() -> OrderUpdated:
        return OrderUpdated(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            quantity=Quantity.from_str("1.5"),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            price=Price.from_str("1500.0"),
            trigger_price=None,
        )

    @staticmethod
    def order_pending_update() -> OrderPendingUpdate:
        return OrderPendingUpdate(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
        )

    @staticmethod
    def order_pending_cancel() -> OrderPendingCancel:
        return OrderPendingCancel(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
        )

    @staticmethod
    def order_modified_rejected():
        return OrderModifyRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=_STUB_UUID4,
            reason="ORDER_DOES_NOT_EXIST",
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_accepted() -> OrderAccepted:
        return OrderAccepted(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_cancel_rejected() -> OrderCancelRejected:
        return OrderCancelRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            reason="ORDER_DOES_NOT_EXIST",
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_canceled() -> OrderCanceled:
        return OrderCanceled(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_expired() -> OrderExpired:
        return OrderExpired(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=_STUB_UUID4,
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_filled(
        order: MarketOrder,
        instrument: CurrencyPair | CryptoPerpetual | CryptoFuture | Equity,
        strategy_id: StrategyId | None = None,
        account_id: AccountId | None = None,
        venue_order_id: VenueOrderId | None = None,
        trade_id: TradeId | None = None,
        position_id: PositionId | None = None,
        last_qty: Quantity | None = None,
        last_px: Price | None = None,
        liquidity_side: LiquiditySide = LiquiditySide.TAKER,
        ts_filled_ns: int = 0,
        account: CashAccount | None = None,
    ) -> OrderFilled:
        if strategy_id is None:
            strategy_id = order.strategy_id
        if account_id is None:
            account_id = order.account_id
            if account_id is None:
                account_id = TestIdProviderPyo3.account_id()
        if venue_order_id is None:
            venue_order_id = VenueOrderId("1")
        if trade_id is None:
            trade_id = TradeId(order.client_order_id.value.replace("O", "E"))
        if position_id is None:
            position_id = order.position_id
        if last_px is None:
            last_px = Price.from_str(f"{1:.{instrument.price_precision}f}")
        if last_qty is None:
            last_qty = order.quantity
        if account is None:
            from nautilus_trader.test_kit.rust.accounting_pyo3 import TestAccountingProviderPyo3

            account = TestAccountingProviderPyo3.cash_account()
        assert account is not None
        commission = account.calculate_commission(
            instrument=instrument,
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )
        return OrderFilled(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            trade_id=trade_id,
            position_id=position_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px or order.price or Price.from_str("1.00000"),
            currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            ts_event=ts_filled_ns,
            event_id=TestIdProviderPyo3.uuid(),
            ts_init=0,
            reconciliation=False,
        )
