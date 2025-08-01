
class Account:
    """
    The base class for all trading accounts.
    """

    id: AccountId
    type: AccountType
    base_currency: Currency | None
    is_cash_account: bool
    is_margin_account: bool
    calculate_account_state: bool
    _events: list[AccountState]
    _commissions: dict[Currency, Money]
    _balances: dict[Currency, AccountBalance]
    _balances_starting: dict[Currency, Money]

    def __init__(self, event: AccountState, calculate_account_state: bool) -> None: ...
    def __eq__(self, other: Account) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def last_event(self) -> AccountState:
        """
        Return the accounts last state event.

        Returns
        -------
        AccountState

        """
        ...
    @property
    def events(self) -> list[AccountState]:
        """
        Return all events received by the account.

        Returns
        -------
        list[AccountState]

        """
        ...
    @property
    def event_count(self) -> int:
        """
        Return the count of events.

        Returns
        -------
        int

        """
        ...
    def currencies(self) -> list[Currency]:
        """
        Return the account currencies.

        Returns
        -------
        list[Currency]

        """
        ...
    def starting_balances(self) -> dict[Currency, Money]:
        """
        Return the account starting balances.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def balances(self) -> dict[Currency, AccountBalance]:
        """
        Return the account balances totals.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def balances_total(self) -> dict[Currency, Money]:
        """
        Return the account balances totals.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def balances_free(self) -> dict[Currency, Money]:
        """
        Return the account balances free.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def balances_locked(self) -> dict[Currency, Money]:
        """
        Return the account balances locked.

        Returns
        -------
        dict[Currency, Money]

        """
        ...
    def commissions(self) -> dict[Currency, Money]:
        """
        Return the total commissions for the account.
        """
        ...
    def balance(self, currency: Currency | None = None) -> AccountBalance | None:
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        AccountBalance or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        ...
    def balance_total(self, currency: Currency | None = None) -> Money | None:
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        ...
    def balance_free(self, currency: Currency | None = None) -> Money | None:
        """
        Return the account balance free.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        ...
    def balance_locked(self, currency: Currency | None = None) -> Money | None:
        """
        Return the account balance locked.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If ``None`` then will use the default
            currency (if set).

        Returns
        -------
        Money or ``None``

        Raises
        ------
        ValueError
            If `currency` is ``None`` and `base_currency` is ``None``.

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        ...
    def commission(self, currency: Currency) -> Money | None:
        """
        Return the total commissions for the given currency.

        Parameters
        ----------
        currency : Currency
            The currency for the commission.

        Returns
        -------
        Money or ``None``

        """
        ...
    def apply(self, event: AccountState) -> None:
        """
        Apply the given account event to the account.

        Parameters
        ----------
        event : AccountState
            The account event to apply.

        Raises
        ------
        ValueError
            If `event.account_type` is not equal to `self.type`.
        ValueError
            If `event.account_id` is not equal to `self.id`.
        ValueError
            If `event.base_currency` is not equal to `self.base_currency`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def update_balances(self, balances: list[AccountBalance]) -> None:
        """
        Update the account balances.

        There is no guarantee that every account currency is included in the
        given balances, therefore we only update included balances.

        Parameters
        ----------
        balances : list[AccountBalance]
            The balances for the update.

        Raises
        ------
        ValueError
            If `balances` is empty.
        AccountBalanceNegative
            If account type is ``CASH``, and balance is negative.

        """
        ...
    def update_commissions(self, commission: Money) -> None:
        """
        Update the commissions.

        Can be negative which represents credited commission.

        Parameters
        ----------
        commission : Money
            The commission to update with.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def purge_account_events(self, ts_now: int, lookback_secs: int = 0) -> None:
        """
        Purge all account state events which are outside the lookback window.

        Guaranteed to retain at least the latest event.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).
        lookback_secs : uint64_t, default 0
            The purge lookback window (seconds) from when the account state event occurred.
            Only events which are outside the lookback window will be purged.
            A value of 0 means purge all account state events.

        """
        ...
    def is_unleveraged(self, instrument_id: InstrumentId) -> bool:
        """
        Return whether the given instrument is leveraged for this account (leverage == 1).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to check.

        Returns
        -------
        bool

        """
        ...
    def calculate_commission(self, instrument: Instrument, last_qty: Quantity, last_px: Price, liquidity_side: LiquiditySide, use_quote_for_inverse: bool = False) -> Money: ...
    def calculate_pnls(self, instrument: Instrument, fill: OrderFilled, position: Position | None = None) -> list: ...
    def balance_impact(self, instrument: Instrument, quantity: Quantity, price: Price, order_side: OrderSide) -> Money: ...
