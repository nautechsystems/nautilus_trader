from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def _build_context() -> dict[str, object]:
    clock = TestClock()
    trader_id = TraderId("TRADER-001")
    account_id = AccountId("SIM-001")
    venue = Venue("SIM")
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=clock)
    exec_engine = ExecutionEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=ExecEngineConfig(debug=True),
    )
    risk_engine = RiskEngine(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=RiskEngineConfig(debug=True),
    )
    exec_client = MockExecutionClient(
        client_id=ClientId(venue.value),
        venue=venue,
        account_type=AccountType.CASH,
        base_currency=AUDUSD_SIM.quote_currency,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    portfolio.update_account(TestEventStubs.cash_account_state(account_id=account_id))
    exec_engine.register_client(exec_client)
    cache.add_instrument(AUDUSD_SIM)
    exec_engine.start()

    strategy = Strategy()
    strategy.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    return {
        "account_id": account_id,
        "cache": cache,
        "clock": clock,
        "exec_engine": exec_engine,
        "portfolio": portfolio,
        "risk_engine": risk_engine,
        "strategy": strategy,
        "venue": venue,
    }


def test_cash_borrowing_capability_without_order_opt_in_still_denies() -> None:
    ctx = _build_context()
    cache = ctx["cache"]
    clock = ctx["clock"]
    exec_engine = ctx["exec_engine"]
    portfolio = ctx["portfolio"]
    risk_engine = ctx["risk_engine"]
    strategy = ctx["strategy"]
    venue = ctx["venue"]
    account_id = ctx["account_id"]

    cache.add_quote_tick(TestDataStubs.quote_tick(AUDUSD_SIM))

    AccountFactory.register_cash_borrowing(venue.value)
    try:
        portfolio.update_account(TestEventStubs.cash_account_state(account_id=account_id))

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            quantity=Quantity.from_int(10_000_000),
        )
        command = SubmitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            order=order,
            command_id=UUID4(),
            ts_init=clock.timestamp_ns(),
        )

        risk_engine.execute(command)

        assert order.status == OrderStatus.DENIED
        assert exec_engine.command_count == 0
    finally:
        AccountFactory.deregister_cash_borrowing(venue.value)


def test_cash_borrowing_requires_capability_and_order_opt_in() -> None:
    ctx = _build_context()
    cache = ctx["cache"]
    clock = ctx["clock"]
    exec_engine = ctx["exec_engine"]
    portfolio = ctx["portfolio"]
    risk_engine = ctx["risk_engine"]
    strategy = ctx["strategy"]
    venue = ctx["venue"]
    account_id = ctx["account_id"]

    cache.add_quote_tick(TestDataStubs.quote_tick(AUDUSD_SIM))

    AccountFactory.register_cash_borrowing(venue.value)
    try:
        portfolio.update_account(TestEventStubs.cash_account_state(account_id=account_id))

        account = cache.account(account_id)
        assert account is not None
        assert account.allow_borrowing is True

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            quantity=Quantity.from_int(10_000_000),
        )
        command = SubmitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            order=order,
            command_id=UUID4(),
            ts_init=clock.timestamp_ns(),
            allow_cash_borrowing=True,
        )

        risk_engine.execute(command)

        assert order.status == OrderStatus.INITIALIZED
        assert exec_engine.command_count == 1
    finally:
        AccountFactory.deregister_cash_borrowing(venue.value)
