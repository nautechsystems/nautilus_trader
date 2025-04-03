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

from decimal import Decimal

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.error import AccountBalanceNegative
from nautilus_trader.accounting.error import AccountMarginExceeded

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.accounts.cash cimport CashAccount
from nautilus_trader.accounting.accounts.margin cimport MarginAccount
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position


cdef class AccountsManager:
    """
    Provides account management functionality.

    Parameters
    ----------
    cache : CacheFacade
        The read-only cache for the manager.
    logger : Logger
        The logger for the manager.
    clock : Clock
        The clock for the manager.
    """

    def __init__(
        self,
        CacheFacade cache not None,
        Logger logger not None,
        Clock clock not None,
    ):
        self._clock = clock
        self._log = logger
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
        if position_id is None:
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

        cdef list pnls = account.calculate_pnls(instrument, fill, position)
        if is_logging_initialized():
            self._log.debug(f"Calculated PnLs: {pnls}")

        # Calculate final PnL including commissions
        cdef Money pnl
        if account.base_currency is not None:
            self._update_balance_single_currency(
                account=account,
                fill=fill,
                pnl=Money(0, account.base_currency) if not pnls else pnls[0],
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
        uint64_t ts_event,
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
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account event occurred.

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
        else:
            raise RuntimeError("invalid `AccountType`")  # pragma: no cover (design-time error)

    cdef AccountState _update_balance_locked(
        self,
        CashAccount account,
        Instrument instrument,
        list orders_open,
        uint64_t ts_event,
    ):
        if not orders_open:
            account.clear_balance_locked(instrument.id)
            return self._generate_account_state(
                account=account,
                ts_event=ts_event,
            )

        total_locked = Decimal(0)
        base_xrate  = Decimal(0)

        cdef Currency currency = instrument.get_cost_currency()
        cdef:
            Order order
        for order in orders_open:
            assert order.instrument_id == instrument.id

            if not order.is_open_c() or (not order.has_price_c() and not order.has_trigger_price_c()):
                # Does not contribute to locked balance
                continue

            # Calculate balance locked
            locked = account.calculate_balance_locked(
                instrument,
                order.side,
                order.quantity,
                order.price if order.has_price_c() else order.trigger_price,
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate == 0:
                    # Cache base currency and xrate
                    currency = account.base_currency
                    base_xrate = self._calculate_xrate_to_base(
                        instrument=instrument,
                        account=account,
                        side=order.side,
                    )

                    if base_xrate == 0:
                        self._log.debug(
                            f"Cannot calculate balance locked: "
                            f"insufficient data for "
                            f"{instrument.get_cost_currency()}/{account.base_currency}"
                        )
                        return None  # Cannot calculate

                # Apply base xrate
                locked = round(locked * base_xrate, currency.get_precision())

            # Increment total locked
            total_locked += locked

        cdef Money locked_money = Money(total_locked, currency)
        account.update_balance_locked(instrument.id, locked_money)

        self._log.info(f"{instrument.id} balance_locked={locked_money.to_formatted_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

    cdef AccountState _update_margin_init(
        self,
        MarginAccount account,
        Instrument instrument,
        list orders_open,
        uint64_t ts_event,
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
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        AccountState or ``None``

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(orders_open, "orders_open")

        total_margin_init = Decimal(0)
        base_xrate = Decimal(0)

        cdef Currency currency = instrument.get_cost_currency()

        cdef Order order
        for order in orders_open:
            assert order.instrument_id == instrument.id, f"order not for instrument {instrument}"

            if not order.is_open_c() or (not order.has_price_c() and not order.has_trigger_price_c()):
                # Does not contribute to initial margin
                continue

            # Calculate initial margin
            margin_init = account.calculate_margin_init(
                instrument,
                order.quantity,
                order.price if order.has_price_c() else order.trigger_price,
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate == 0:
                    # Cache base currency and xrate
                    currency = account.base_currency
                    base_xrate = self._calculate_xrate_to_base(
                        instrument=instrument,
                        account=account,
                        side=order.side,
                    )

                    if base_xrate == 0:
                        self._log.debug(
                            f"Cannot calculate initial (order) margin: "
                            f"insufficient data for "
                            f"{instrument.get_cost_currency()}/{account.base_currency}"
                        )
                        return None  # Cannot calculate

                # Apply base xrate
                margin_init = round(margin_init * base_xrate, currency.get_precision())

            # Increment total initial margin
            total_margin_init += margin_init

        cdef Money margin_init_money = Money(total_margin_init, currency)
        if total_margin_init == 0:
            account.clear_margin_init(instrument.id)
        else:
            account.update_margin_init(instrument.id, margin_init_money)

        self._log.info(f"{instrument.id} margin_init={margin_init_money.to_formatted_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

    cdef AccountState update_positions(
        self,
        MarginAccount account,
        Instrument instrument,
        list positions_open,
        uint64_t ts_event,
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
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account event occurred.

        Returns
        -------
        AccountState or ``None``

        """
        Condition.not_none(account, "account")
        Condition.not_none(instrument, "instrument")
        Condition.not_none(positions_open, "positions_open")

        total_margin_maint = Decimal(0)
        base_xrate = Decimal(0)

        cdef Currency currency = instrument.get_cost_currency()

        cdef Position position
        for position in positions_open:
            assert position.instrument_id == instrument.id

            if not position.is_open_c():
                # Does not contribute to maintenance margin
                continue

            # Calculate margin
            margin_maint = account.calculate_margin_maint(
                instrument,
                position.side,
                position.quantity,
                instrument.make_price(position.avg_px_open),
            ).as_decimal()

            if account.base_currency is not None:
                if base_xrate == 0:
                    # Cache base currency and xrate
                    currency = account.base_currency
                    base_xrate = self._calculate_xrate_to_base(
                        instrument=instrument,
                        account=account,
                        side=position.entry,
                    )

                    if base_xrate == 0:
                        self._log.debug(
                            f"Cannot calculate maintenance (position) margin: "
                            f"insufficient data for "
                            f"{instrument.get_cost_currency()}/{account.base_currency}"
                        )
                        return None  # Cannot calculate

                # Apply base xrate
                margin_maint = round(margin_maint * base_xrate, currency.get_precision())

            # Increment total maintenance margin
            total_margin_maint += margin_maint

        cdef Money margin_maint_money = Money(total_margin_maint, currency)
        if total_margin_maint == 0:
            account.clear_margin_maint(instrument.id)
        else:
            account.update_margin_maint(instrument.id, margin_maint_money)

        self._log.info(f"{instrument.id} margin_maint={margin_maint_money.to_formatted_str()}")

        return self._generate_account_state(
            account=account,
            ts_event=ts_event,
        )

    cdef void _update_balance_single_currency(
        self,
        Account account,
        OrderFilled fill,
        Money pnl,
    ):
        cdef Money commission = fill.commission
        cdef list balances = []

        if commission.currency != account.base_currency:
            xrate = self._cache.get_xrate(
                venue=fill.instrument_id.venue,
                from_currency=fill.commission.currency,
                to_currency=account.base_currency,
                price_type=PriceType.BID if fill.order_side is OrderSide.SELL else PriceType.ASK,
            )
            if xrate is None:
                self._log.error(
                    f"Cannot calculate account state: "
                    f"insufficient data for "
                    f"{fill.commission.currency}/{account.base_currency}"
                )
                return  # Cannot calculate

            # Convert to account base currency
            commission = Money(commission.as_f64_c() * xrate, account.base_currency)

        if pnl.currency != account.base_currency:
            xrate = self._cache.get_xrate(
                venue=fill.instrument_id.venue,
                from_currency=pnl.currency,
                to_currency=account.base_currency,
                price_type=PriceType.BID if fill.order_side is OrderSide.SELL else PriceType.ASK,
            )
            if xrate is None:
                self._log.error(
                    f"Cannot calculate account state: "
                    f"insufficient data for "
                    f"{pnl.currency}/{account.base_currency}"
                )
                return  # Cannot calculate

            # Convert to account base currency
            pnl = Money(pnl.as_f64_c() * xrate, account.base_currency)

        pnl = pnl.sub(commission)
        if pnl._mem.raw == 0:
            return  # Nothing to adjust

        cdef AccountBalance balance = account.balance()
        if balance is None:
            self._log.error(f"Cannot complete transaction: no balance for {pnl.currency}")
            return

        # Calculate new balance
        cdef AccountBalance new_balance = AccountBalance(
            total=balance.total.add(pnl),
            locked=balance.locked,
            free=balance.free.add(pnl),
        )
        balances.append(new_balance)

        # Finally update balances and commission
        account.update_balances(balances)
        account.update_commissions(commission)

    cdef void _update_balance_multi_currency(
        self,
        Account account,
        OrderFilled fill,
        list pnls,
    ):
        cdef list balances = []

        cdef Money commission = fill.commission
        cdef AccountBalance balance = None
        cdef AccountBalance new_balance = None
        cdef bint apply_commission = commission._mem.raw != 0

        cdef:
            Money pnl
            Money total
            Money free
        for pnl in pnls:
            if apply_commission and pnl.currency == commission.currency:
                # Deduct the commission from the realized PnL (the commission may also be negative)
                pnl = pnl.sub(commission)
                # Ensure we only apply commission once
                apply_commission = False

            if pnl._mem.raw == 0:
                continue  # No adjustment

            currency = pnl.currency
            balance = account.balance(currency)
            if balance is None:
                if pnl._mem.raw < 0:
                    self._log.error(
                        "Cannot complete transaction: "
                        f"no {pnl.currency} to deduct a {pnl.to_formatted_str()} realized PnL from"
                    )
                    return
                new_balance = AccountBalance(
                    total=pnl,
                    locked=Money(0, pnl.currency),
                    free=pnl,
                )
            else:
                new_total = balance.total.as_decimal() + pnl.as_decimal()
                new_free = balance.free.as_decimal() + pnl.as_decimal()
                total = Money(new_total, pnl.currency)
                free = Money(new_free, pnl.currency)
                if new_total < 0:
                    raise AccountBalanceNegative(
                        balance=total.as_decimal(),
                        currency=pnl.currency,
                    )
                if new_free < 0:
                    raise AccountMarginExceeded(
                        balance=total.as_decimal(),
                        margin=balance.locked.as_decimal(),
                        currency=pnl.currency,
                    )

                # Calculate new balance
                new_balance = AccountBalance(
                    total=total,
                    locked=balance.locked,
                    free=free,
                )

            balances.append(new_balance)

        if apply_commission:
            # We still need to apply the commission
            currency = commission.currency
            balance = account.balance(commission.currency)
            if balance is None:
                if commission._mem.raw > 0:
                    self._log.error(
                        f"Cannot complete transaction: no {commission.currency} "
                        f"balance to deduct a {commission.to_formatted_str()} commission from"
                    )
                    return
                balance = AccountBalance(
                    total=Money(0, commission.currency),
                    locked=Money(0, commission.currency),
                    free=Money(0, commission.currency),
                )
            commission_dec = commission.as_decimal()
            balance.total = Money(balance.total.as_decimal() - commission_dec, commission.currency)
            balance.free = Money(balance.free.as_decimal() - commission_dec, commission.currency)
            balances.append(balance)

        if not balances:
            return  # No adjustment

        # Finally update balances and commissions
        account.update_balances(balances)
        account.update_commissions(commission)

    cdef AccountState _generate_account_state(self, Account account, uint64_t ts_event):
        # Generate event
        return AccountState(
            account_id=account.id,
            account_type=account.type,
            base_currency=account.base_currency,
            reported=False,
            balances=list(account.balances().values()),
            margins=list(account.margins().values()) if account.is_margin_account else [],
            info={},
            event_id=UUID4(),
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

        return Decimal(self._cache.get_xrate(
            venue=instrument.id.venue,
            from_currency=instrument.get_cost_currency(),
            to_currency=account.base_currency,
            price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
        ) or 0.0)  # Retain original behavior of returning zero for now
