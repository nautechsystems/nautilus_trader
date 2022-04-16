# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

from nautilus_trader.accounting.error import AccountBalanceNegative

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.accounts.cash cimport CashAccount
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
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position


cdef class AccountsManager:
    """
    Provides account management services for a ``Portfolio``.

    Parameters
    ----------
    cache : CacheFacade
        The read-only cache for the manager.
    log : LoggerAdapter
        The logger for the manager.
    clock : Clock
        The clock for the manager.
    """

    def __init__(
        self,
        CacheFacade cache not None,
        LoggerAdapter log not None,
        Clock clock not None,
    ):
        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = log
        self._cache = cache

    cdef AccountState update_balances(
        self,
        Account account,
        Instrument instrument,
        OrderFilled fill,
    ):
        """
        Update the account balances based on the given fill event.

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
        AccountState or ``None``

        Raises
        ------
        AccountBalanceNegative
            If a new free balance would become negative.

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
        cdef Position position = self._cache.position(position_id)
        # *** position could still be None here ***

        cdef list pnls = account.calculate_pnls(instrument, position, fill)

        # Calculate final PnL
        if account.base_currency is not None:
            # Check single-currency PnLs
            assert len(pnls) == 1, f"{pnls[0]} {pnls[1]}"
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

        return self._generate_account_state(
            account=account,
            ts_event=fill.ts_event,
        )

    cdef AccountState update_orders(
        self,
        Account account,
        Instrument instrument,
        list orders_open,
        int64_t ts_event,
    ):
        """
        Update the account states based on the given orders.

        Parameters
        ----------
        account : MarginAccount
            The account to update.
        instrument : Instrument
            The instrument for the update.
        orders_open : list[Order]
            The open orders for the update.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        AccountState

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(orders_open, "orders_open")

        if account.is_cash_account:
            return self._update_balance_locked(
                account,
                instrument,
                orders_open,
                ts_event,
            )
        elif account.is_margin_account:
            return self._update_margin_init(
                account,
                instrument,
                orders_open,
                ts_event,
            )
        else:  # pragma: no cover (design-time error)
            raise RuntimeError("invalid account type")

    cdef AccountState _update_balance_locked(
        self,
        CashAccount account,
        Instrument instrument,
        list orders_open,
        int64_t ts_event,
    ):
        if not orders_open:
            account.clear_balance_locked(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=ts_event,
            )

        total_locked: Decimal = Decimal(0)
        base_xrate: Optional[Decimal] = None

        cdef Currency currency = instrument.get_cost_currency()
        cdef Order order
        for order in orders_open:
            assert order.instrument_id == instrument.id
            assert order.is_open_c()

            # Calculate balance locked
            locked: Decimal = account.calculate_balance_locked(
                instrument,
                order.side,
                order.quantity,
                order.price,
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate is not None:
                    locked *= base_xrate
                    return

                currency = account.base_currency
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=order.side,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate balance locked: "
                        f"insufficient data for "
                        f"{instrument.get_cost_currency()}/{account.base_currency}."
                    )
                    return None  # Cannot calculate

                base_xrate = xrate  # Cache xrate
                locked *= base_xrate  # Apply xrate

            # Increment total locked
            total_locked += locked

        cdef Money locked_money = Money(total_locked, currency)
        account.update_balance_locked(instrument.id, locked_money)

        self._log.info(f"{instrument.id} balance_locked={locked_money.to_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

    cdef AccountState _update_margin_init(
        self,
        MarginAccount account,
        Instrument instrument,
        list orders_open,
        int64_t ts_event,
    ):
        """
        Update the initial (order) margin for margin accounts or locked balance
        for cash accounts.

        Will return ``None`` if operation fails.

        Parameters
        ----------
        account : MarginAccount
            The account to update.
        instrument : Instrument
            The instrument for the update.
        orders_open : list[Order]
            The open orders for the update.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        AccountState or ``None``

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(orders_open, "orders_open")

        if not orders_open:
            account.clear_margin_init(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=ts_event,
            )

        total_margin_init: Decimal = Decimal(0)
        base_xrate: Optional[Decimal] = None

        cdef Currency currency = instrument.get_cost_currency()
        cdef Order order
        for order in orders_open:
            assert order.instrument_id == instrument.id
            assert order.is_open_c()

            # Calculate initial margin
            margin_init: Decimal = account.calculate_margin_init(
                instrument,
                order.quantity,
                order.price if order.has_price_c() else order.trigger_price,
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate is not None:
                    margin_init *= base_xrate
                    return

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

                base_xrate = xrate  # Cache xrate
                margin_init *= base_xrate  # Apply xrate

            # Increment total initial margin
            total_margin_init += margin_init

        cdef Money margin_init_money = Money(total_margin_init, currency)
        account.update_margin_init(instrument.id, margin_init_money)

        # self._log.info(f"{instrument.id} margin_init={margin_init_money.to_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

    cdef AccountState update_positions(
        self,
        MarginAccount account,
        Instrument instrument,
        list positions_open,
        int64_t ts_event,
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
            The open positions for the update.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        AccountState or ``None``

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(positions_open, "positions_open")

        if not positions_open:
            account.clear_margin_maint(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=ts_event,
            )

        total_margin_maint: Decimal = Decimal(0)
        base_xrate: Optional[Decimal] = None

        cdef Currency currency = instrument.get_cost_currency()
        cdef Position position
        for position in positions_open:
            assert position.instrument_id == instrument.id
            assert position.is_open_c()

            # Calculate margin
            margin_maint: Decimal = account.calculate_margin_maint(
                instrument,
                position.side,
                position.quantity,
                position.avg_px_open,
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate is not None:
                    margin_maint *= base_xrate
                    return

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

                base_xrate = xrate  # Cache xrate
                margin_maint *= base_xrate  # Apply xrate

            # Increment total maintenance margin
            total_margin_maint += margin_maint

        cdef Money margin_maint_money = Money(total_margin_maint, currency)
        account.update_margin_maint(instrument.id, margin_maint_money)

        # self._log.info(f"{instrument.id} margin_maint={margin_maint_money.to_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

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
                price_type=PriceType.BID if fill.order_side is OrderSide.SELL else PriceType.ASK,
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
                price_type=PriceType.BID if fill.order_side is OrderSide.SELL else PriceType.ASK,
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

        # Calculate new balances
        new_total: Decimal = balance.total + pnl
        new_free: Decimal = balance.free + pnl

        # Validate free balance
        if new_free < 0:
            raise AccountBalanceNegative(balance=new_free)

        cdef AccountBalance new_balance = AccountBalance(
            total=Money(new_total, account.base_currency),
            locked=balance.locked,
            free=Money(new_free, account.base_currency),
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
            if commission.currency != currency and commission.as_decimal() != 0:
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
                    total=Money(pnl, currency),
                    locked=Money(0, currency),
                    free=Money(pnl, currency),
                )
            else:
                # Calculate new balances
                new_total: Decimal = balance.total + pnl
                new_free: Decimal = balance.free + pnl

                # Validate free balance
                if new_free < 0:
                    raise AccountBalanceNegative(balance=new_free)

                new_balance = AccountBalance(
                    total=Money(new_total, currency),
                    locked=balance.locked,
                    free=Money(new_free, currency),
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
            margins=list(account.margins().values()) if account.is_margin_account else [],
            info={},
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

    cdef object _calculate_xrate_to_base(
        self,
        Account account,
        Instrument instrument,
        OrderSide side,
    ):
        if account.base_currency is None:
            return Decimal(1)  # No conversion needed
        else:
            return self._cache.get_xrate(
                venue=instrument.id.venue,
                from_currency=instrument.get_cost_currency(),
                to_currency=account.base_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )
