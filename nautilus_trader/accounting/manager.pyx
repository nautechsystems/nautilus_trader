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

from decimal import Decimal

from nautilus_trader.accounting.accounts.margin cimport MarginAccount
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.position cimport Position


cdef class AccountsManager:
    """
    Provides account management services for a ``Portfolio``.
    """

    def __init__(
        self,
        CacheFacade cache not None,
        LoggerAdapter log not None,
        Clock clock not None,
    ):
        """
        Initialize a new instance of the ``AccountsManager`` class.

        Parameters
        ----------
        cache : CacheFacade
            The read-only cache for the manager.

        """
        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = log
        self._cache = cache

    cdef AccountState update_margin_initial(
        self,
        Account account,
        Instrument instrument,
        list passive_orders_working,
    ):
        """
        Update the initial (order) margin for margin accounts or locked balance
        for cash accounts.

        Will return ``None`` if operation fails.

        Parameters
        ----------
        account : Account
            The account to update.
        instrument : Instrument
            The instrument for the update.
        passive_orders_working : list[PassiveOrder]

        Returns
        -------
        AccountState or None

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(passive_orders_working, "orders_working")
        Condition.list_type(passive_orders_working, PassiveOrder, "orders_working")

        if not passive_orders_working:
            account.clear_margin_initial(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=account.last_event_c().ts_event,
            )

        total_margin_initial: Decimal = Decimal(0)
        cdef Currency currency = instrument.get_cost_currency()

        cdef PassiveOrder order
        for order in passive_orders_working:
            assert order.instrument_id == instrument.id

            # Calculate initial margin
            margin_initial = account.calculate_margin_initial(
                instrument,
                order.quantity,
                order.price,
            )

            if account.base_currency is not None:
                currency = account.base_currency
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=order.side,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate initial (order) margin: "
                        f"insufficient data for "
                        f"{instrument.get_cost_currency()}/{account.base_currency}."
                    )
                    return None  # Cannot calculate

                margin_initial *= xrate

            # Increment total initial margin
            total_margin_initial += margin_initial

        cdef Money margin_initial_money = Money(total_margin_initial, currency)
        account.update_margin_initial(instrument.id, margin_initial_money)

        self._log.info(f"{instrument.id} margin_initial={margin_initial_money.to_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=account.last_event_c().ts_event,
        )

    cdef AccountState update_margin_maint(
        self,
        MarginAccount account,
        Instrument instrument,
        list positions_open,
    ):
        """
        Update the maintenance (position) margin.

        Will return ``None`` if operation fails.

        Parameters
        ----------
        account : Account
            The account to update.
        instrument : Instrument
            The instrument for the update.
        positions_open : list[Position]

        Returns
        -------
        AccountState or None

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(positions_open, "positions_open")

        if not positions_open:
            account.clear_margin_maint(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=account.last_event_c().ts_event,
            )

        total_margin_maint: Decimal = Decimal(0)
        cdef Currency currency = instrument.get_cost_currency()

        cdef Position position
        for position in positions_open:
            assert position.instrument_id == instrument.id

            # Calculate margin
            margin_maint = account.calculate_margin_maint(
                instrument,
                position.side,
                position.quantity,
                position.avg_px_open,
            )

            if account.base_currency is not None:
                currency = account.base_currency
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate maintenance (position) margin: "
                        f"insufficient data for "
                        f"{instrument.get_cost_currency()}/{account.base_currency})."
                    )
                    return None  # Cannot calculate

                margin_maint *= xrate

            # Increment total maintenance margin
            total_margin_maint += margin_maint

        cdef Money margin_maint_money = Money(total_margin_maint, currency)
        account.update_margin_maint(instrument.id, margin_maint_money)

        self._log.info(f"{instrument.id} margin_maint={margin_maint_money.to_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=account.last_event_c().ts_event,
        )

    cdef AccountState update_balances(
        self,
        Account account,
        Instrument instrument,
        OrderFilled fill,
    ):
        """
        Update the maintenance (position) margin.

        Will return ``None`` if operation fails.

        Parameters
        ----------
        account : Account
            The account to update.
        instrument : Instrument
            The instrument for the update.
        fill : OrderFilled
            The order filled event for the update

        Returns
        -------
        AccountState or None

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        # Determine any position
        cdef PositionId position_id = fill.position_id
        if fill.position_id is None:
            # Check for open positions
            positions_open = self._cache.positions_open(
                venue=None,  # Faster query filtering
                instrument_id=fill.instrument_id,
            )
            if positions_open:
                position_id = positions_open[0].id

        # Determine any position
        cdef Position position = None
        if position_id is not None:
            position = self._cache.position(position_id)
        # *** position could still be None here ***

        cdef list pnls = account.calculate_pnls(instrument, position, fill)

        # Calculate final PnL
        if account.base_currency is not None:
            # Check single-currency PnLs
            assert len(pnls) == 1
            self._update_balance_single_currency(
                account=account,
                fill=fill,
                pnl=pnls[0],
            )
        else:
            self._update_balance_multi_currency(
                account=account,
                fill=fill,
                pnls=pnls,
            )

        return self._generate_account_state(account, fill.ts_event)

    cdef void _update_balance_single_currency(
        self,
        Account account,
        OrderFilled fill,
        Money pnl,
    ) except *:
        cdef Money commission = fill.commission
        cdef list balances = []
        if commission.currency != account.base_currency:
            xrate: Decimal = self._cache.get_xrate(
                venue=fill.instrument_id.venue,
                from_currency=fill.commission.currency,
                to_currency=account.base_currency,
                price_type=PriceType.BID if fill.side is OrderSide.SELL else PriceType.ASK,
            )
            if xrate == 0:
                self._log.error(
                    f"Cannot calculate account state: "
                    f"insufficient data for "
                    f"{fill.commission.currency}/{account.base_currency}."
                )
                return  # Cannot calculate

            # Convert to account base currency
            commission = Money(commission * xrate, account.base_currency)

        if pnl.currency != account.base_currency:
            xrate: Decimal = self._cache.get_xrate(
                venue=fill.instrument_id.venue,
                from_currency=pnl.currency,
                to_currency=account.base_currency,
                price_type=PriceType.BID if fill.side is OrderSide.SELL else PriceType.ASK,
            )
            if xrate == 0:
                self._log.error(
                    f"Cannot calculate account state: "
                    f"insufficient data for "
                    f"{pnl.currency}/{account.base_currency}."
                )
                return  # Cannot calculate

            # Convert to account base currency
            pnl = Money(pnl * xrate, account.base_currency)

        pnl = Money(pnl - commission, account.base_currency)
        if pnl.as_decimal() == 0:
            return  # Nothing to adjust

        cdef AccountBalance balance = account.balance()
        cdef AccountBalance new_balance = AccountBalance(
            currency=account.base_currency,
            total=Money(balance.total + pnl, account.base_currency),
            locked=balance.locked,
            free=Money(balance.free + pnl, account.base_currency),
        )
        balances.append(new_balance)

        # Finally update balances
        account.update_balances(balances)

    cdef void _update_balance_multi_currency(
        self,
        Account account,
        OrderFilled fill,
        list pnls,
    ) except *:
        cdef list balances = []

        cdef Money commission = fill.commission
        cdef AccountBalance balance = None
        cdef AccountBalance new_balance = None
        cdef Money pnl
        for pnl in pnls:
            currency = pnl.currency
            if commission.currency != currency and commission.as_decimal() > 0:
                balance = account.balance(commission.currency)
                if balance is None:
                    self._log.error(
                        "Cannot calculate account state: "
                        f"no cached balances for {currency}."
                    )
                    return
                balance.total = Money(balance.total - commission, currency)
                balance.free = Money(balance.free - commission, currency)
                balances.append(balance)
            else:
                pnl = Money(pnl - commission, currency)

            if not balances and pnl.as_decimal() == 0:
                return  # No adjustment

            balance = account.balance(currency)
            if balance is None:
                if pnl.as_decimal() < 0:
                    self._log.error(
                        "Cannot calculate account state: "
                        f"no cached balances for {currency}."
                    )
                    return
                new_balance = AccountBalance(
                    currency=currency,
                    total=Money(pnl, currency),
                    locked=Money(0, currency),
                    free=Money(pnl, currency),
                )
            else:
                new_balance = AccountBalance(
                    currency=currency,
                    total=Money(balance.total + pnl, currency),
                    locked=balance.locked,
                    free=Money(balance.free + pnl, currency),
                )

            balances.append(new_balance)

        # Finally update balances
        account.update_balances(balances)

    cdef AccountState _generate_account_state(self, Account account, int64_t ts_event):
        # Generate event
        return AccountState(
            account_id=account.id,
            account_type=account.type,
            base_currency=account.base_currency,
            reported=False,
            balances=list(account.balances().values()),
            info={},
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

    cdef object _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side):
        if account.base_currency is not None:
            return self._cache.get_xrate(
                venue=instrument.id.venue,
                from_currency=instrument.get_cost_currency(),
                to_currency=account.base_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )

        return Decimal(1)  # No conversion needed
